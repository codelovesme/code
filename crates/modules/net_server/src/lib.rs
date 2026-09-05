//! The `net_server` native module — particles in, particles out, no protocol.
//!
//! The other half of [`net_client`](../net_client). Where `http_server`
//! speaks HTTP — paths, methods, status codes — this speaks nothing. It
//! accepts a **particle**, hands it to the program's own handlers, and sends
//! whatever they returned back to the sender. A caller needs a url and a
//! particle; there is no path to design and no method to choose.
//!
//! Three handlers, the same shape `http_server` settled on:
//!
//! - `Config { port?, host?, max_particle_bytes?, response_timeout_seconds?,
//!   allow_origin? }`
//!   → `ConfigResult { ok }` — the setup particle. Optional (the defaults are
//!   loopback and an OS-chosen port); an `Exception` after `Listen`.
//! - `Listen {}` → `ListenResult { ok, port, message }` — binds and starts
//!   serving. Takes no fields.
//! - `Stop {}` → `StopResult { ok }` — stops the accept thread, which is what
//!   lets the program end (see below).
//!
//! Pushed into the program: **the particle the sender sent**, its `_class`
//! intact, plus two fields this module adds — `app`, from the url's path
//! segment, and `_request_id`, which is how an answer finds its way back.
//!
//! ```code
//! link "net_server.so" as net
//!
//! Ping { value } => {
//!     return Pong { value = value }
//! }
//!
//! emit Config { port = 9000 } to net get _
//! emit Listen { } to net get l
//! assert l.ok
//! ```
//!
//! That is the whole program — no keep-alive loop. `Listen` starts a thread,
//! this module answers `code_module_serving` while it is alive, and the host
//! keeps the program up for exactly that long. `Stop {}` ends it.
//!
//! **No authentication, no authorization, no policy.** Deliberately: a token
//! check belongs where the user and their permissions can be read, which is
//! a handler, in `code`. So this module carries an envelope it never opens.
//! The pattern that makes that work is a chain of handlers, which the
//! language already allows (see `handlers.rs`: a handler may emit to another,
//! as long as the call graph stays acyclic):
//!
//! ```code
//! Impulse { token, particle } => {
//!     emit Decode { token = token } to jwt get who
//!     if not who.valid { return Denied { reason = "bad token" } }
//!
//!     | The allow-list: the classes this program offers, and what each needs.
//!     if particle._class = "Ping" {
//!         emit DoPing { user = who.sub } to this get r
//!         return r
//!     }
//!     return Unknown { class = particle._class }
//! }
//! ```
//!
//! **Name the classes you offer; do not emit what arrived.** `emit particle
//! to this` would hand a sender the whole program: the class is theirs to
//! choose, so they could reach any handler in it — `Log`, `Exception`,
//! anything internal — and pick which one runs. The allow-list is what keeps
//! this transport from being a dispatch table into a program.
//!
//! It is also why this module knows nothing about euglena: no manifest, no
//! projects directory, no per-app public-class list. The old `server`
//! organelle read all three because it *was* a euglena organelle. This one is
//! a `code` module, and the policy it would have enforced is a program's to
//! write.
//!
//! **The wire is framed, not HTTP.** A four-byte big-endian length, then that
//! many bytes of JSON. Nothing about it suggests a protocol to a reader, which
//! is the point: `net_client` and `net_server` are two ends of one pipe, not
//! an implementation of somebody else's standard. JSON because the language's
//! value model *is* JSON's six kinds, so a particle crosses without a
//! translation layer to argue with.
//!
//! **Many in flight, answered one at a time.** Every connection gets its own
//! thread and its own slot in `PENDING`, so a sender never waits for the
//! socket. Dispatch into the program stays serial regardless — the host
//! drains on one thread, and a handler may not re-enter another — so the
//! program chews through them in order. That is a deliberate split: the
//! socket does not block, and the language's single-threaded guarantee is
//! untouched.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use code_native::*;
use serde_json::Value as Json;

/// Frame body cap when `Config` doesn't say — 1 MiB, the same default
/// `http_server` puts on a request body.
const DEFAULT_MAX_PARTICLE_BYTES: f64 = 1_048_576.0;
/// How long a connection waits for the program's answer before giving up and
/// answering itself. A program that defined no handler for the class it was
/// sent must not leave a socket open forever.
const DEFAULT_RESPONSE_TIMEOUT_SECONDS: f64 = 10.0;

/// One `Listen` per module, because there is one program to serve.
static LISTENING: OnceLock<()> = OnceLock::new();

/// Whether the accept thread is still running — the answer
/// `code_module_serving` gives the host, and so what decides how long the
/// program lives.
static SERVING: AtomicBool = AtomicBool::new(false);
/// Set by `Stop`, read by the accept loop between connections.
static STOPPING: AtomicBool = AtomicBool::new(false);
/// Connections accepted but not yet answered — counted from just before the
/// serving thread is spawned to the moment it has written its frame back.
///
/// This is the other half of "is this module still serving". `SERVING` alone
/// answers whether *new* connections are being taken, and `Stop` turns that
/// off from inside a handler — which is exactly the handler that still owes
/// its caller a reply. Without this the host could see nothing serving and
/// end the program between the answer being produced and it reaching the
/// socket, which is precisely what `Quit { } => { emit Stop {} …
/// return Bye {} }` asks for.
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// Decrements [`IN_FLIGHT`] however its scope is left. `serve` returns from
/// six places; a manual decrement would be five chances to leak a count and
/// hold the program open for ever.
struct InFlight;

impl InFlight {
    fn enter() -> Self {
        IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
        InFlight
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}
/// Where the server is listening, so `Stop` can reach it: `accept()` blocks
/// and there is no portable way to interrupt it, so `Stop` opens a connection
/// of its own to wake the loop.
static BOUND_ADDR: Mutex<Option<std::net::SocketAddr>> = Mutex::new(None);

/// What the server binds and serves as. Set (optionally) by `Config`, read
/// once by `Listen`.
static CONFIG: Mutex<Option<ServerConfig>> = Mutex::new(None);

/// Connections waiting for the program's answer, by request id.
///
/// A map rather than `http_server`'s single slot: this module accepts
/// concurrently, so several senders can be waiting at once even though the
/// program answers them one at a time.
static PENDING: Mutex<Option<HashMap<u64, Sender<Json>>>> = Mutex::new(None);
/// Hands out request ids. Monotonic, never reused within a process.
static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// The field carrying the request id on a pushed particle.
///
/// Underscore-prefixed, like `_class`, because it is this module's
/// bookkeeping rather than the sender's data — and it has to travel *on* the
/// particle: `code_module_inbound_reply` hands back the particle it is
/// answering, and with an arbitrary `_class` on the wire there is nothing
/// else to correlate on. `http_server` could check `_class == "Request"`
/// because it only ever pushes one class; this one pushes whatever it was
/// sent.
const REQUEST_ID_FIELD: &str = "_request_id";

#[derive(Clone)]
struct ServerConfig {
    host: String,
    port: u16,
    max_bytes: usize,
    timeout: f64,
    /// What a browser is told about who may read the answer. Open by
    /// default — see `write_response` for why an origin check here would be
    /// an answer to a question this module does not ask.
    allow_origin: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        // Loopback: a module that opened a program to the network the moment
        // it was linked would be making that decision for its caller. Port 0
        // asks the OS for a free one, and `ListenResult.port` reports which.
        ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 0,
            max_bytes: DEFAULT_MAX_PARTICLE_BYTES as usize,
            timeout: DEFAULT_RESPONSE_TIMEOUT_SECONDS,
            allow_origin: "*".to_string(),
        }
    }
}

#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

declare_inbound!();
declare_inbound_reply!(answered);

/// # Safety
///
/// Called by the host between the program's statements; touches only atomics.
///
/// Non-zero while the accept thread is alive. This is what holds the program
/// open past its last statement, the way a non-daemon thread holds a JVM
/// open, and it is why a program using this module writes no keep-alive loop.
/// See `code_abi.h` item 8.
#[no_mangle]
pub extern "C" fn code_module_serving() -> std::ffi::c_int {
    let accepting = SERVING.load(Ordering::SeqCst);
    let owed = IN_FLIGHT.load(Ordering::SeqCst) > 0;
    i32::from(accepting || owed)
}

/// # Safety
///
/// Both pointers must be valid for the duration of the call and laid out per
/// `code_abi.h` — the host guarantees this on every dispatch.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "net_server", |out| {
        match read_field_str(particle, "_class") {
            Some("Config") => handle_config(out, particle),
            Some("Listen") => handle_listen(out),
            Some("Stop") => handle_stop(out),
            // Including whatever class this module pushed, which it answers
            // through `code_module_inbound_reply` rather than a dispatch.
            _ => null(out),
        }
    })
}

/// The program's answer to a particle this module pushed — the handler's
/// return value, or a null value when no handler matched.
///
/// Correlated by [`REQUEST_ID_FIELD`], which this module put on the particle
/// on the way in. A particle without it was pushed by something else (or by a
/// future version of this module) and is not an answer to a connection.
fn answered(particle: &CodeValue, result: &CodeValue) {
    let Some(id) = find_field(particle, REQUEST_ID_FIELD).and_then(read_number) else {
        return;
    };
    let sender = {
        let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        pending.as_mut().and_then(|map| map.remove(&(id as u64)))
    };
    let Some(sender) = sender else {
        // The connection timed out and answered itself while the program was
        // still thinking. Late, not wrong.
        report_log("Warn", "an answer arrived after its sender had given up");
        return;
    };
    // A null result means nothing handled the class. That is a real answer and
    // the sender is told so plainly, rather than being left to time out.
    let payload = if result.tag == CodeTag::Null {
        Json::Null
    } else {
        to_json(result)
    };
    let _ = sender.send(payload);
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `Config { port?, host?, max_particle_bytes?, response_timeout_seconds?,
/// allow_origin? }`
/// → `ConfigResult { ok }`. Optional; an `Exception` once `Listen` has run,
/// because the socket is already bound and a later change would be a lie.
fn handle_config(out: &mut CodeValue, particle: &CodeValue) {
    if LISTENING.get().is_some() {
        exception(
            out,
            "net_server",
            "Config after Listen — the socket is already bound",
        );
        return;
    }

    let mut cfg = ServerConfig::default();
    if let Some(host) = read_field_str(particle, "host") {
        cfg.host = host.to_string();
    }
    if let Some(port) = find_field(particle, "port").and_then(read_number) {
        if port.fract() != 0.0 || !(0.0..=65535.0).contains(&port) {
            exception(
                out,
                "net_server",
                "port must be a whole number in 0..=65535",
            );
            return;
        }
        cfg.port = port as u16;
    }
    if let Some(max) = find_field(particle, "max_particle_bytes").and_then(read_number) {
        if max <= 0.0 || !max.is_finite() {
            exception(out, "net_server", "max_particle_bytes must be positive");
            return;
        }
        cfg.max_bytes = max as usize;
    }
    if let Some(t) = find_field(particle, "response_timeout_seconds").and_then(read_number) {
        if t <= 0.0 || !t.is_finite() {
            exception(
                out,
                "net_server",
                "response_timeout_seconds must be positive",
            );
            return;
        }
        cfg.timeout = t;
    }
    if let Some(origin) = find_field(particle, "allow_origin").and_then(read_str) {
        if origin.is_empty() {
            exception(
                out,
                "net_server",
                "allow_origin must name an origin, or \"*\" for any",
            );
            return;
        }
        cfg.allow_origin = origin.to_string();
    }

    *CONFIG.lock().unwrap_or_else(|e| e.into_inner()) = Some(cfg);

    let mut buf = SlotBuffer::new(2);
    borrowed_str(buf.slot_mut(0), c"ConfigResult");
    boolean(buf.slot_mut(1), true);
    object(out, &[c"_class", c"ok"], &mut buf);
    buf.release_all();
}

/// `Listen {}` → `ListenResult { ok, port, message }`. Binds the socket and
/// spawns the accept thread; takes no fields, since what it binds comes from
/// `Config` or its defaults.
fn handle_listen(out: &mut CodeValue) {
    if LISTENING.set(()).is_err() {
        listen_result(out, false, 0, "already listening");
        return;
    }
    let cfg = CONFIG
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_default();

    let listener = match TcpListener::bind((cfg.host.as_str(), cfg.port)) {
        Ok(l) => l,
        Err(e) => {
            let message = format!("cannot listen on {}:{}: {e}", cfg.host, cfg.port);
            report_exception(&message);
            listen_result(out, false, 0, &message);
            return;
        }
    };
    let addr = listener.local_addr().ok();
    let bound = addr.map(|a| a.port()).unwrap_or(cfg.port);
    *BOUND_ADDR.lock().unwrap_or_else(|e| e.into_inner()) = addr;
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(HashMap::new());

    // Set before the spawn, so there is no window in which the program could
    // reach its last statement and exit out from under a thread that is
    // already listening.
    SERVING.store(true, Ordering::SeqCst);
    // Read before the move, not through the lock again afterwards.
    let host = cfg.host.clone();
    std::thread::spawn(move || accept_loop(listener, cfg));
    report_log("Info", &format!("listening on {host}:{bound}"));
    listen_result(out, true, bound, "");
}

/// `Stop {}` → `StopResult { ok }` — stop serving, and so let the program end.
///
/// This is how a program that writes no keep-alive loop shuts itself down.
/// `accept()` blocks with no portable interrupt, so this sets the flag and
/// then connects to the server's own address: the loop wakes for it, sees the
/// flag, and leaves without serving it.
fn handle_stop(out: &mut CodeValue) {
    if LISTENING.get().is_none() {
        exception(out, "net_server", "Stop before Listen — nothing is serving");
        return;
    }
    STOPPING.store(true, Ordering::SeqCst);
    if let Some(addr) = *BOUND_ADDR.lock().unwrap_or_else(|e| e.into_inner()) {
        // Best effort: if the connection cannot be made the listener is
        // already gone, which is the state `Stop` was asking for.
        let _ = TcpStream::connect(addr);
    }

    let mut buf = SlotBuffer::new(2);
    borrowed_str(buf.slot_mut(0), c"StopResult");
    boolean(buf.slot_mut(1), true);
    object(out, &[c"_class", c"ok"], &mut buf);
    buf.release_all();
}

fn listen_result(out: &mut CodeValue, ok: bool, port: u16, message: &str) {
    let mut buf = SlotBuffer::new(4);
    borrowed_str(buf.slot_mut(0), c"ListenResult");
    boolean(buf.slot_mut(1), ok);
    number(buf.slot_mut(2), port as f64);
    owned_str(buf.slot_mut(3), message);
    object(out, &[c"_class", c"ok", c"port", c"message"], &mut buf);
    buf.release_all();
}

// ---------------------------------------------------------------------------
// The server thread
// ---------------------------------------------------------------------------

fn accept_loop(listener: TcpListener, cfg: ServerConfig) {
    for stream in listener.incoming() {
        // Checked first: `Stop`'s own connection is what woke this iteration,
        // and it is not a sender to serve.
        if STOPPING.load(Ordering::SeqCst) {
            break;
        }
        match stream {
            Ok(stream) => {
                // One thread per connection, so a sender never waits for the
                // socket. What it *does* wait for is the program, which
                // answers one particle at a time no matter how many are here.
                let cfg = cfg.clone();
                // Counted here rather than inside `serve`, so a connection is
                // never invisible in the gap between `spawn` and the thread
                // actually starting.
                let in_flight = InFlight::enter();
                std::thread::spawn(move || {
                    let _in_flight = in_flight;
                    serve(stream, cfg)
                });
            }
            Err(e) => report_log("Warn", &format!("accept failed: {e}")),
        }
    }
    // The last thing this thread does. Once it is false nothing holds the
    // program open any more, so this must happen after the loop, never before.
    SERVING.store(false, Ordering::SeqCst);
    report_log("Info", "stopped listening");
}

/// One connection, start to finish: read the request, hand the particle to
/// the program, wait for the answer, write it back.
fn serve(mut stream: TcpStream, cfg: ServerConfig) {
    // Bound the read. A client that connects and then says nothing would
    // block for ever, and since this connection is counted in `IN_FLIGHT` a
    // stalled one would hold the whole program open — `Stop` included. The
    // same `timeout` that bounds waiting for the program bounds waiting for
    // the sender.
    let _ = stream.set_read_timeout(Some(Duration::from_secs_f64(cfg.timeout)));

    let request = match read_request(&mut stream, cfg.max_bytes) {
        Ok(request) => request,
        Err(message) => {
            report_log("Warn", &format!("bad request: {message}"));
            let _ = write_response(&mut stream, 400, &cfg, Some(&error_json(&message)));
            return;
        }
    };

    // A browser asks before it sends: same-origin is the default, and a page
    // on another origin has to be told this door is open to it. Answered
    // before anything else, because a preflight carries no body and means no
    // work.
    if request.method == "OPTIONS" {
        let _ = write_response(&mut stream, 204, &cfg, None);
        return;
    }
    if request.method != "POST" {
        let message = format!(
            "this door takes POST, not {} — a particle is sent, not fetched",
            request.method
        );
        let _ = write_response(&mut stream, 405, &cfg, Some(&error_json(&message)));
        return;
    }

    // **The body is the particle**, and the path is the app. Nothing wraps
    // anything: a sender writes what it wants to say and nothing else, and
    // `curl -d '{"_class":"Ping"}' http://host:9000/ping-api` is a whole
    // request. The app reaches the program as a field, so a runtime hosting
    // several can route on it.
    let particle: Json = match serde_json::from_slice(&request.body) {
        Ok(json) => json,
        Err(e) => {
            let message = format!("body is not JSON: {e}");
            report_log("Warn", &message);
            let _ = write_response(&mut stream, 400, &cfg, Some(&error_json(&message)));
            return;
        }
    };
    if !particle.is_object() {
        let message = "body is not a particle — a particle is an object".to_string();
        report_log("Warn", &message);
        let _ = write_response(&mut stream, 400, &cfg, Some(&error_json(&message)));
        return;
    }
    if particle.get("_class").and_then(Json::as_str).is_none() {
        let message = "particle has no `_class`".to_string();
        report_log("Warn", &message);
        let _ = write_response(&mut stream, 400, &cfg, Some(&error_json(&message)));
        return;
    }

    let id = REQUEST_ID.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = channel();
    {
        let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(map) = pending.as_mut() {
            map.insert(id, tx);
        }
    }

    push_particle(&particle, &request.app, id);

    match rx.recv_timeout(Duration::from_secs_f64(cfg.timeout)) {
        Ok(answer) => {
            // 200 whatever the handler said. A `Denied` is an answer, not a
            // transport failure, and a sender reads it by its class like any
            // other particle — the status line is about whether the *door*
            // worked.
            let _ = write_response(&mut stream, 200, &cfg, Some(&answer));
        }
        Err(_) => {
            // Drop the slot first, so a late answer is told nobody is waiting
            // rather than sending into a dead channel.
            let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(map) = pending.as_mut() {
                map.remove(&id);
            }
            drop(pending);
            let message = format!("the program did not answer within {}s", cfg.timeout);
            report_exception(&message);
            let _ = write_response(&mut stream, 504, &cfg, Some(&error_json(&message)));
        }
    }
}

// ---------------------------------------------------------------------------
// The wire: HTTP, one POST, the body is the particle
//
// It used to be a four-byte length and then that many bytes of JSON. Smaller
// on the wire and simpler to read, and it had one fatal property: **a browser
// cannot speak it.** A browser opens no raw sockets — HTTP and WebSocket are
// all it has — so a framing of our own meant no application in a page could
// ever reach a program, whatever else it could reach.
//
// HTTP costs a few hundred bytes per request and buys the rest of the world
// with them: a proxy in front, TLS terminated by something that already knows
// how, a request visible in a browser's devtools, and `curl` when something is
// wrong. The custom frame was invisible to all of it.
//
// What did *not* change is the shape, which is what this module is for: a
// particle arrives, the program's handlers answer it, the answer goes back.
// There is still no path to design and no method to choose. HTTP here is only
// how the bytes travel.
//
// Written by hand rather than shared with `http_server`, whose job is the
// opposite one — giving a program paths, methods and status codes to answer
// with. What is needed here is one method at one path with one content type,
// which is a small enough slice that having it twice is cheaper than having a
// crate between them.
// ---------------------------------------------------------------------------

/// How much of a request may be headers. Generous for anything a sender here
/// would send, and a bound, because a client that never sends the blank line
/// would otherwise be read for ever.
const MAX_HEAD_BYTES: usize = 16 * 1024;

struct Request {
    method: String,
    /// The url's path segment, which names the app — `/ping-api` is
    /// `"ping-api"`, and `/` is `""`. Never more than one segment deep:
    /// there is nothing further to say, since a particle already says what it
    /// wants by its class.
    app: String,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream, max_bytes: usize) -> Result<Request, String> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    // Byte at a time until the blank line. A request head is small and read
    // once per connection, and reading further would mean buffering part of a
    // body this function has not yet decided it will accept.
    loop {
        match stream.read(&mut byte) {
            Ok(0) => return Err("the sender closed before finishing its request".to_string()),
            Ok(_) => head.push(byte[0]),
            Err(e) => return Err(format!("cannot read the request: {e}")),
        }
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
        if head.len() > MAX_HEAD_BYTES {
            return Err(format!("request head is over {MAX_HEAD_BYTES} bytes"));
        }
    }

    let head = String::from_utf8_lossy(&head).into_owned();
    let mut lines = head.split("\r\n");
    let start = lines.next().unwrap_or_default();
    let mut parts = start.split(' ');
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default();
    if method.is_empty() || target.is_empty() {
        return Err("not an HTTP request".to_string());
    }
    // Query and fragment are dropped rather than refused: a browser or a
    // proxy may add either, and neither means anything here.
    let path = target.split(['?', '#']).next().unwrap_or("/");
    let app = path.trim_matches('/').split('/').next().unwrap_or("");

    let mut length = 0usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    if length > max_bytes {
        return Err(format!(
            "body is {length} bytes, over the {max_bytes}-byte max_particle_bytes"
        ));
    }

    let mut body = vec![0u8; length];
    if length > 0 {
        stream
            .read_exact(&mut body)
            .map_err(|e| format!("body is shorter than its content-length: {e}"))?;
    }

    Ok(Request {
        method,
        app: app.to_string(),
        body,
    })
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    cfg: &ServerConfig,
    value: Option<&Json>,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        405 => "Method Not Allowed",
        504 => "Gateway Timeout",
        _ => "Error",
    };
    let body = value
        .map(|v| serde_json::to_vec(v).unwrap_or_else(|_| b"null".to_vec()))
        .unwrap_or_default();

    let mut head = format!("HTTP/1.1 {status} {reason}\r\n");
    head.push_str("content-type: application/json\r\n");
    head.push_str(&format!("content-length: {}\r\n", body.len()));
    // What a browser has to be told before it will let a page read the
    // answer. Configurable, and open by default, because this door has no
    // notion of who is knocking: it carries a token it never opens, and the
    // handler that reads that token is where "who is allowed to ask for this"
    // is decided. An origin check here would look like an answer to that
    // question without being one.
    head.push_str(&format!(
        "access-control-allow-origin: {}\r\n",
        cfg.allow_origin
    ));
    head.push_str("access-control-allow-headers: content-type\r\n");
    head.push_str("access-control-allow-methods: POST, OPTIONS\r\n");
    head.push_str("access-control-max-age: 86400\r\n");
    // One request per connection. Keep-alive would mean tracking which
    // connections are idle and which are mid-request, and every sender here
    // asks one question at a time.
    head.push_str("connection: close\r\n\r\n");

    stream.write_all(head.as_bytes())?;
    if !body.is_empty() {
        stream.write_all(&body)?;
    }
    stream.flush()
}

/// What a sender is told when the request never reached a handler. An
/// `Exception` particle, so the far side reads it the same way it reads any
/// other failure in this language.
fn error_json(message: &str) -> Json {
    let mut map = serde_json::Map::new();
    map.insert("_class".to_string(), Json::String("Exception".to_string()));
    map.insert("source".to_string(), Json::String("net_server".to_string()));
    map.insert("message".to_string(), Json::String(message.to_string()));
    Json::Object(map)
}

// ---------------------------------------------------------------------------
// Pushing into the program
// ---------------------------------------------------------------------------

/// Push the sender's particle, with `app` and `_request_id` added.
fn push_particle(particle: &Json, app: &str, id: u64) {
    let mut value = CodeValue::zeroed();
    from_json(&mut value, particle);

    // Rebuild with the two extra fields rather than mutating: a `CodeValue`
    // object is a fixed pair of key and value buffers, so adding a field means
    // building the object again.
    let existing: Vec<(String, CodeValue)> = object_entries(&value)
        .filter(|(k, _)| *k != "app" && *k != REQUEST_ID_FIELD)
        .map(|(k, v)| {
            let mut copied = CodeValue::zeroed();
            copy(&mut copied, v);
            (k.to_string(), copied)
        })
        .collect();

    let mut keys: Vec<&str> = existing.iter().map(|(k, _)| k.as_str()).collect();
    keys.push("app");
    keys.push(REQUEST_ID_FIELD);

    let mut buf = SlotBuffer::new(keys.len());
    for (i, (_, v)) in existing.iter().enumerate() {
        copy(buf.slot_mut(i as i64), v);
    }
    owned_str(buf.slot_mut(existing.len() as i64), app);
    number(buf.slot_mut(existing.len() as i64 + 1), id as f64);

    let mut particle_value = CodeValue::zeroed();
    object_dyn(&mut particle_value, &keys, &mut buf);
    buf.release_all();

    emit_inbound(&particle_value);

    release(&mut particle_value);
    release(&mut value);
    for (_, mut v) in existing {
        release(&mut v);
    }
}

/// Push `Log { source, level, message }` — the common particle. Best effort in
/// both directions: the host may have taken no inbound channel, and the
/// program may have no handler. Neither is this module's problem.
fn report_log(level: &str, message: &str) {
    let mut particle = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(4);
    borrowed_str(buf.slot_mut(0), c"Log");
    borrowed_str(buf.slot_mut(1), c"net_server");
    owned_str(buf.slot_mut(2), level);
    owned_str(buf.slot_mut(3), message);
    object(
        &mut particle,
        &[c"_class", c"source", c"level", c"message"],
        &mut buf,
    );
    buf.release_all();
    emit_inbound(&particle);
    release(&mut particle);
}

/// Push `Exception { source, message }` — the common particle.
fn report_exception(message: &str) {
    let mut particle = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(3);
    borrowed_str(buf.slot_mut(0), c"Exception");
    borrowed_str(buf.slot_mut(1), c"net_server");
    owned_str(buf.slot_mut(2), message);
    object(&mut particle, &[c"_class", c"source", c"message"], &mut buf);
    buf.release_all();
    emit_inbound(&particle);
    release(&mut particle);
}

// ---------------------------------------------------------------------------
// CodeValue <-> JSON
// ---------------------------------------------------------------------------

/// A code value to its JSON form, **`_class` included**.
///
/// The one difference from the `json` module's `Stringify`, which drops it:
/// there, `_class` is the language's bookkeeping and a caller asking for JSON
/// wants their data. Here the class *is* the payload — it is what decides
/// which handler runs on the far side — so dropping it would be dropping the
/// address off an envelope.
fn to_json(v: &CodeValue) -> Json {
    match v.tag {
        CodeTag::Number => number_to_json(v.number),
        CodeTag::Str => Json::String(read_str(v).unwrap_or_default().to_owned()),
        CodeTag::Bool => Json::Bool(read_bool(v).unwrap_or(false)),
        CodeTag::Null => Json::Null,
        CodeTag::Array => Json::Array(array_elems(v).map(to_json).collect()),
        CodeTag::Object => {
            let mut map = serde_json::Map::new();
            for (key, value) in object_entries(v) {
                // The request id is this module's own, and the far side has no
                // use for it.
                if key != REQUEST_ID_FIELD {
                    map.insert(key.to_owned(), to_json(value));
                }
            }
            Json::Object(map)
        }
    }
}

/// A JSON value written into `out` as a code value.
fn from_json(out: &mut CodeValue, v: &Json) {
    match v {
        Json::Null => null(out),
        Json::Bool(b) => boolean(out, *b),
        Json::Number(n) => number(out, n.as_f64().unwrap_or(0.0)),
        Json::String(s) => owned_str(out, s),
        Json::Array(items) => {
            let mut buf = SlotBuffer::new(items.len());
            for (i, item) in items.iter().enumerate() {
                from_json(buf.slot_mut(i as i64), item);
            }
            array(out, &mut buf);
            buf.release_all();
        }
        Json::Object(map) => {
            let keys: Vec<&str> = map.keys().map(String::as_str).collect();
            let mut buf = SlotBuffer::new(map.len());
            for (i, value) in map.values().enumerate() {
                from_json(buf.slot_mut(i as i64), value);
            }
            object_dyn(out, &keys, &mut buf);
            buf.release_all();
        }
    }
}

/// A code Number (always `f64`) to JSON: a whole value in `i64` range writes
/// as an integer, matching the language's own "shortest form that
/// round-trips" rule; everything else is a float. A non-finite value has no
/// JSON spelling, so it becomes `null` rather than failing the whole frame.
fn number_to_json(n: f64) -> Json {
    if !n.is_finite() {
        Json::Null
    } else if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
        Json::Number((n as i64).into())
    } else {
        serde_json::Number::from_f64(n).map_or(Json::Null, Json::Number)
    }
}

//! The `http_server` native module — the other half of `http_client`.
//!
//! Modelled on `euglena-language`'s `server` organelle, which settled the
//! shape: every request becomes a particle pushed *up* into the program, and
//! the program answers it. Cut down to what a language without cells needs:
//! no JWT, no impulse routing, no per-app public-class policy — those are a
//! gateway's business, and a gateway can be written in this language on top
//! of this module.
//!
//! Two handlers — configuration and the action are separate:
//!
//! - `Config { port?, host?, max_body_bytes?, response_timeout_seconds? }`
//!   → `ConfigResult { ok }` — the setup particle. Optional (the defaults
//!   are loopback, an OS-chosen port); an `Exception` if sent after `Listen`.
//! - `Listen {}` → `ListenResult { ok, port, message }` — binds the socket
//!   and starts serving. Takes no fields.
//!
//! Pushed into the program (its own handlers, between statements):
//!
//! - `Request { method, path, query, body, headers }` — `headers` is an
//!   object keyed by lowercased header name: `req.headers.host`, or
//!   `req.headers["content-type"]` for a hyphenated one
//! - `Log { source, level, message }` and `Exception { source, message }`,
//!   the common particles — see the root README.
//!
//! **The handler's return value is the response.** A pushed particle's answer
//! comes back through `code_module_inbound_reply` (see `code_abi.h`), so
//! there is no `Respond` particle and no request id in the program's hands:
//! a request is answered the way every other particle in this language is
//! answered, by returning one.
//!
//! ```code
//! link "http_server.so" as srv
//!
//! Request { method, path } => {
//!     return Response { status = 200, body = "hi from $path" }
//! }
//!
//! emit Config { port = 8080 } to srv get _
//! emit Listen { } to srv get l
//! assert l.ok
//! loop {
//! }
//! ```
//!
//! A handler that returns something without a `status`/`body` still answers —
//! 200 and an empty body. Returning null, or defining no `Request` handler at
//! all, is a 404: nobody claimed the request.
//!
//! **One request at a time, on purpose.** The accept loop handles a
//! connection to completion before taking the next, because the program it
//! serves is single-threaded: a pushed particle is dispatched by the host's
//! drain, one at a time, and a handler may not re-enter another. Accepting
//! concurrently would only queue work the program cannot start any sooner —
//! and would let one slow client's request overtake another's in the ring.
//!
//! That is also what makes the pending request a single slot rather than a
//! map: when an answer arrives there is exactly one request it can belong to,
//! so nothing has to be correlated and no id has to exist.
//!
//! **A program cannot call its own server.** `emit Get … to http` blocks
//! inside `http_client`, and the drain that would dispatch the `Request` only
//! runs between the program's own statements — so a self-request waits for a
//! handler that cannot start until the request finishes. That is a property
//! of a single-threaded program, not a bug here; `tests/http_server_module.rs`
//! makes its requests from outside the process.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use code_native::*;

/// Response body cap when `Listen` doesn't say — 1 MiB, the same default
/// `http_client` puts on a response it reads.
const DEFAULT_MAX_BODY_BYTES: f64 = 1_048_576.0;
/// How long a connection waits for the program's `Respond` before answering
/// 504 itself. A program that forgot to handle `Request`, or whose handler
/// failed, must not leave a socket open forever.
const DEFAULT_RESPONSE_TIMEOUT_SECONDS: f64 = 10.0;
/// The one request waiting for an answer. The accept thread puts the sending
/// half here and blocks on the receiver; the reply from the program takes it
/// out and sends. One slot rather than a map because the accept loop is
/// serial — see the note at the top about why that is not a limitation.
static PENDING: Mutex<Option<Sender<Reply>>> = Mutex::new(None);

/// One `Listen` per module, because there is one program to serve.
static LISTENING: OnceLock<()> = OnceLock::new();

/// What the server binds and serves as. Set (optionally) by `Config`, read
/// once by `Listen`. `None` means "nobody sent `Config`" — the defaults
/// below apply.
static CONFIG: Mutex<Option<ServerConfig>> = Mutex::new(None);

#[derive(Clone)]
struct ServerConfig {
    host: String,
    port: u16,
    max_body: usize,
    timeout: f64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        // Loopback: a module that opened a program to the network the moment
        // it was linked would be making that decision for its caller. Port 0
        // asks the OS for a free one, and `ListenResult.port` reports which.
        ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 0,
            max_body: DEFAULT_MAX_BODY_BYTES as usize,
            timeout: DEFAULT_RESPONSE_TIMEOUT_SECONDS,
        }
    }
}

struct Reply {
    status: u16,
    body: String,
    content_type: String,
}

#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

declare_inbound!();
declare_inbound_reply!(answered);

/// The program's answer to a `Request` this module pushed — the handler's
/// return value, or a null value when no handler matched.
///
/// This is the whole response path. A `Response { status?, body?,
/// content_type? }` is answered as written; anything else that is a particle
/// still counts as an answer (200, empty body), since returning *something*
/// is the program saying it handled the request. Null is not an answer, and
/// becomes a 404 — that is what "no handler for this class" looks like from
/// here, and it is the honest status for it.
fn answered(particle: &CodeValue, result: &CodeValue) {
    // Only replies to a `Request`. This module pushes `Log` and `Exception`
    // too, and every push gets an answer — so without this check a `Log`
    // whose answer found no pending request would log a warning, which is
    // another push, which gets another answer: a feedback loop that spins the
    // program forever. Found by writing it.
    if read_field_str(particle, "_class") != Some("Request") {
        return;
    }
    let sender = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take();
    let Some(sender) = sender else {
        // The request timed out and answered itself while the program was
        // still thinking. Late, not wrong.
        report_log("Warn", "an answer arrived after the request had timed out");
        return;
    };
    let reply = if result.tag == CodeTag::Null {
        Reply {
            status: 404,
            body: String::new(),
            content_type: "text/plain; charset=utf-8".to_string(),
        }
    } else {
        Reply {
            status: optional_number(result, "status", 200.0) as u16,
            body: find_field(result, "body")
                .map(value_text)
                .unwrap_or_default(),
            content_type: match find_field(result, "content_type").map(value_text) {
                Some(ct) if !ct.is_empty() => ct,
                _ => "text/plain; charset=utf-8".to_string(),
            },
        }
    };
    let _ = sender.send(reply);
}

/// # Safety
///
/// Both pointers must be valid for the duration of the call and laid out per
/// `code_abi.h` — the host guarantees this on every dispatch.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "http_server", |out| {
        match read_field_str(particle, "_class") {
            Some("Config") => handle_config(out, particle),
            Some("Listen") => handle_listen(out),
            // Including `Request`, which this module pushes but never answers.
            _ => null(out),
        }
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `Config { port?, host?, max_body_bytes?, response_timeout_seconds? }` →
/// `ConfigResult { ok }`. Optional — `Listen` uses [`ServerConfig::default`]
/// if nobody sends it — but every field it does carry is validated here
/// rather than silently coerced later. Sending it after `Listen` is an
/// `Exception`: the socket is already bound, so the config would be a lie.
fn handle_config(out: &mut CodeValue, particle: &CodeValue) {
    if LISTENING.get().is_some() {
        exception(out, "http_server", "Config has no effect after Listen");
        return;
    }
    let mut cfg = ServerConfig::default();

    if let Some(host) = find_field(particle, "host").and_then(read_str) {
        if !host.is_empty() {
            cfg.host = host.to_string();
        }
    }
    if let Some(v) = find_field(particle, "port") {
        match read_number(v) {
            Some(n) if n.fract() == 0.0 && (0.0..=65535.0).contains(&n) => cfg.port = n as u16,
            _ => {
                exception(
                    out,
                    "http_server",
                    "'port' must be a whole number in 0..=65535",
                );
                return;
            }
        }
    }
    if let Some(v) = find_field(particle, "max_body_bytes") {
        match read_number(v) {
            Some(n) if n.fract() == 0.0 && n >= 0.0 => cfg.max_body = n as usize,
            _ => {
                exception(
                    out,
                    "http_server",
                    "'max_body_bytes' must be a whole number, 0 or more",
                );
                return;
            }
        }
    }
    if let Some(v) = find_field(particle, "response_timeout_seconds") {
        match read_number(v) {
            Some(n) if n.is_finite() && n > 0.0 => cfg.timeout = n,
            _ => {
                exception(
                    out,
                    "http_server",
                    "'response_timeout_seconds' must be a positive number",
                );
                return;
            }
        }
    }

    *CONFIG.lock().unwrap_or_else(|e| e.into_inner()) = Some(cfg);

    let mut buf = SlotBuffer::new(2);
    borrowed_str(buf.slot_mut(0), c"ConfigResult");
    boolean(buf.slot_mut(1), true);
    object(out, &[c"_class", c"ok"], &mut buf);
    buf.release_all();
}

/// `Listen {}` → `ListenResult { ok, port, message }`. The "start serving"
/// action, distinct from `Config` — it takes no fields: whatever the server
/// binds and serves as comes from `Config` (or its defaults). Binds the
/// socket and spawns the accept thread.
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
    let bound = listener.local_addr().map(|a| a.port()).unwrap_or(cfg.port);

    // The thread outlives this call, which is the whole point: a server that
    // only ran while the program was inside `emit` would never accept
    // anything. Nothing joins it — the process ending is what stops it, and
    // the host leaves a module that can push loaded for exactly that reason.
    std::thread::spawn(move || accept_loop(listener, cfg.max_body, cfg.timeout));
    report_log("Info", &format!("listening on {}:{bound}", cfg.host));
    listen_result(out, true, bound, "");
}

// ---------------------------------------------------------------------------
// The server thread
// ---------------------------------------------------------------------------

fn accept_loop(listener: TcpListener, max_body: usize, response_timeout: f64) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => serve(stream, max_body, response_timeout),
            Err(e) => report_log("Warn", &format!("accept failed: {e}")),
        }
    }
}

/// One request, start to finish: read it, hand it to the program, wait for
/// the answer, write it back.
fn serve(mut stream: TcpStream, max_body: usize, response_timeout: f64) {
    let request = match read_request(&mut stream, max_body) {
        Ok(request) => request,
        Err(message) => {
            report_log("Warn", &format!("bad request: {message}"));
            write_response(&mut stream, 400, "text/plain; charset=utf-8", &message);
            return;
        }
    };

    let (tx, rx) = channel();
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);

    push_request(&request);

    match rx.recv_timeout(Duration::from_secs_f64(response_timeout)) {
        Ok(reply) => write_response(&mut stream, reply.status, &reply.content_type, &reply.body),
        Err(_) => {
            // Clear the slot first, so an answer arriving late is told that
            // nobody is waiting rather than sending into a dead channel.
            *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = None;
            report_exception(&format!(
                "the program did not answer within {response_timeout}s"
            ));
            write_response(
                &mut stream,
                504,
                "text/plain; charset=utf-8",
                "the program did not answer in time",
            );
        }
    }
}

struct Request {
    method: String,
    path: String,
    query: String,
    body: String,
    /// Header name (lowercased — HTTP names are case-insensitive) to value,
    /// in arrival order. A name that repeats is joined with `", "`, the
    /// field-value merge RFC 9110 §5.3 allows.
    headers: Vec<(String, String)>,
}

/// Deliberately not a complete HTTP parser: the request line, the one header
/// that decides how many body bytes follow, and those bytes. Everything else
/// a real deployment needs — TLS, keep-alive, chunked bodies, HTTP/2 — is
/// what a reverse proxy in front of this is for, and saying so is more honest
/// than a half-implementation that looks like it handles them.
fn read_request(stream: &mut TcpStream, max_body: usize) -> Result<Request, String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);

    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|e| e.to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    if method.is_empty() || target.is_empty() {
        return Err("malformed request line".to_string());
    }
    // `?` splits the path from the query, and the query is handed over as the
    // raw string. Parsing it into an object would mean deciding what repeated
    // keys mean and how to decode `%20`, which is a `url` module's job.
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_string(), query.to_string()),
        None => (target, String::new()),
    };

    let mut content_length = 0usize;
    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if read == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue; // a continuation line or garbage — not a header we can name
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_string();
        if name == "content-length" {
            content_length = value.parse().unwrap_or(0);
        }
        match headers.iter_mut().find(|(n, _)| *n == name) {
            Some((_, existing)) => {
                existing.push_str(", ");
                existing.push_str(&value);
            }
            None => headers.push((name, value)),
        }
    }
    if content_length > max_body {
        return Err(format!("request body exceeds max_body_bytes ({max_body})"));
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    }
    Ok(Request {
        method,
        path,
        query,
        // Lossy rather than refusing: this language's only string is UTF-8,
        // and a request that is nearly text should still reach the program.
        body: String::from_utf8_lossy(&body).to_string(),
        headers,
    })
}

fn write_response(stream: &mut TcpStream, status: u16, content_type: &str, body: &str) {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        504 => "Gateway Timeout",
        _ => "Status",
    };
    // `Connection: close` because there is no keep-alive here: one request
    // per connection is what a serial accept loop can honestly promise.
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

// ---------------------------------------------------------------------------
// Particles pushed into the program
// ---------------------------------------------------------------------------

/// `Request { method, path, query, body, headers }`.
///
/// `headers` is an object — lowercased name to value (`req.headers.host`,
/// `req.headers."content-type"`) — so an app reads a bearer token or a
/// content type without this module having to know which headers matter.
/// Its keys are built at run time, which `code_object` has owned the bytes
/// of since 2026-08-29; the same fix is what lets `http_client` grow
/// response headers when it needs them.
fn push_request(request: &Request) {
    let mut headers = CodeValue::zeroed();
    {
        let mut hbuf = SlotBuffer::new(request.headers.len());
        for (i, (_, value)) in request.headers.iter().enumerate() {
            owned_str(hbuf.slot_mut(i as i64), value);
        }
        let names: Vec<&str> = request.headers.iter().map(|(n, _)| n.as_str()).collect();
        object_dyn(&mut headers, &names, &mut hbuf);
        hbuf.release_all();
    }

    let mut particle = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(6);
    borrowed_str(buf.slot_mut(0), c"Request");
    owned_str(buf.slot_mut(1), &request.method);
    owned_str(buf.slot_mut(2), &request.path);
    owned_str(buf.slot_mut(3), &request.query);
    owned_str(buf.slot_mut(4), &request.body);
    copy(buf.slot_mut(5), &headers);
    object(
        &mut particle,
        &[c"_class", c"method", c"path", c"query", c"body", c"headers"],
        &mut buf,
    );
    buf.release_all();
    release(&mut headers);
    emit_inbound(&particle);
    release(&mut particle);
}

/// Push `Exception { source, message }` — the common particle. Best effort in
/// both directions: the host may have taken no inbound channel, and the
/// program may have no handler. Neither is this module's problem.
fn report_exception(message: &str) {
    let mut particle = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(3);
    borrowed_str(buf.slot_mut(0), c"Exception");
    borrowed_str(buf.slot_mut(1), c"http_server");
    owned_str(buf.slot_mut(2), message);
    object(&mut particle, &[c"_class", c"source", c"message"], &mut buf);
    buf.release_all();
    emit_inbound(&particle);
    release(&mut particle);
}

/// Push `Log { source, level, message }` — same shape `http_client` uses, so
/// one handler in a program serves both.
fn report_log(level: &str, message: &str) {
    let mut particle = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(4);
    borrowed_str(buf.slot_mut(0), c"Log");
    borrowed_str(buf.slot_mut(1), c"http_server");
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

// ---------------------------------------------------------------------------
// Operands
// ---------------------------------------------------------------------------

fn listen_result(out: &mut CodeValue, ok: bool, port: u16, message: &str) {
    let mut buf = SlotBuffer::new(4);
    borrowed_str(buf.slot_mut(0), c"ListenResult");
    boolean(buf.slot_mut(1), ok);
    number(buf.slot_mut(2), port as f64);
    owned_str(buf.slot_mut(3), message);
    object(out, &[c"_class", c"ok", c"port", c"message"], &mut buf);
    buf.release_all();
}

/// An optional non-negative Number, falling back rather than refusing —
/// the same rule `http_client` follows, and for the same reason: a negative
/// duration would panic inside `Duration`, and `guarded` would turn that into
/// an Exception naming a Rust internal rather than anything a caller could
/// act on.
fn optional_number(particle: &CodeValue, name: &str, default: f64) -> f64 {
    match find_field(particle, name).and_then(read_number) {
        Some(n) if n >= 0.0 && n.is_finite() => n,
        _ => default,
    }
}

/// A value as text — the same rendering `http_client` uses for a url or a
/// header. An Object or Array renders empty: `runtime.c` serialises JSON but
/// the ABI does not expose it, and a program that wants a JSON body already
/// has one, since string interpolation (`"$payload"`) renders any value as
/// compact JSON in both output modes.
fn value_text(v: &CodeValue) -> String {
    match read_str(v) {
        Some(s) => s.to_string(),
        None => match read_number(v) {
            Some(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", n as i64),
            Some(n) => format!("{n}"),
            None => match read_bool(v) {
                Some(b) => b.to_string(),
                None => String::new(),
            },
        },
    }
}

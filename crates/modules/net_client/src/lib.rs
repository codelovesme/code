//! The `net_client` native module — send a particle, get a particle back.
//!
//! The other half of [`net_server`](../net_server). One handler, two things
//! to give it: where to send, and what to send.
//!
//! ```
//! Send { url, particle, timeout_ms? } → whatever the far side's handlers returned
//! ```
//!
//! ```code
//! link "net_client.so" as net
//!
//! emit Send {
//!     url = "http://127.0.0.1:9000/ping-api",
//!     particle = Impulse { token = "…", particle = Ping { value = 1 } }
//! } to net get answer
//! assert answer ∈ Pong
//! ```
//!
//! **It does not build the envelope.** Whatever particle the program hands
//! over is what crosses the wire, `_class` and all. A token belongs inside
//! that particle, put there by a handler that knows which token to use — this
//! module never looks. That is the same division `net_server` keeps on the
//! far side: authentication and authorization are a program's business,
//! because that is where a user and their permissions can be read.
//!
//! **The url names a host and an app**, and nothing else:
//!
//! ```text
//! http://127.0.0.1:9000/ping-api
//! └ scheme ┘└─ host:port ┘└─ app ┘
//! ```
//!
//! No path beyond the app segment, no method, no query. There is nothing to
//! design: a particle already says what it wants by its class. The app
//! segment is optional and reaches the far side as a field, so a runtime
//! hosting several apps can route on it; a program serving only itself can
//! leave it off.
//!
//! **The wire is HTTP**: one POST, the body is the particle, the path is the
//! app. So `curl -d '{"_class":"Ping"}' http://127.0.0.1:9000/ping-api` is a
//! whole request, and a browser can send one too — which a framing of our own
//! made impossible. See `net_server`'s own docs for the rest of why.
//!
//! **`Send` blocks**, like `http_client`'s handlers do, and bounds the block:
//! `timeout_ms` defaults rather than waiting forever, because nothing in the
//! ABI can stop a module that blocks with no deadline. A refused connection,
//! a timeout or a malformed answer all come back as an `Exception` particle —
//! a value the program can read — never as a dead program.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use code_native::*;
use serde_json::Value as Json;

/// How long `Send` waits when the caller doesn't say. Bounded on purpose:
/// nothing in the ABI can stop a module that blocks forever.
const DEFAULT_TIMEOUT_MS: f64 = 10_000.0;

/// Answer cap — a far side that returns more than this is refused rather than
/// read into memory unbounded. The same 1 MiB `net_server` puts on a frame.
const MAX_ANSWER_BYTES: usize = 1_048_576;

/// The scheme the url carries. Named rather than a protocol, because there is
/// The url is an address on the network and says so. It used to spell itself
/// `euglena://`, which named the shape rather than the transport — and a
/// scheme that lies about the transport is a url nothing else can be handed:
/// not `curl`, not a browser, not a proxy.
const SCHEME: &str = "http://";

#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// # Safety
///
/// Both pointers must be valid for the duration of the call and laid out per
/// `code_abi.h` — the host guarantees this on every dispatch.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "net_client", |out| {
        match read_field_str(particle, "_class") {
            Some("Send") => handle_send(out, particle),
            _ => null(out),
        }
    })
}

/// `Send { url, particle, timeout_ms? }` → the particle the far side's
/// handlers returned.
fn handle_send(out: &mut CodeValue, incoming: &CodeValue) {
    let Some(url) = read_field_str(incoming, "url") else {
        exception(out, "net_client", "Send needs a `url` string");
        return;
    };
    let Some(payload) = find_field(incoming, "particle") else {
        exception(out, "net_client", "Send needs a `particle`");
        return;
    };
    if payload.tag != CodeTag::Object || read_field_str(payload, "_class").is_none() {
        exception(
            out,
            "net_client",
            "`particle` must be a particle — an object with a class",
        );
        return;
    }

    let (host_port, app) = match split_url(url) {
        Ok(parts) => parts,
        Err(message) => {
            exception(out, "net_client", &message);
            return;
        }
    };

    let timeout_ms = match find_field(incoming, "timeout_ms").and_then(read_number) {
        Some(ms) if ms > 0.0 && ms.is_finite() => ms,
        Some(_) => {
            exception(out, "net_client", "timeout_ms must be a positive number");
            return;
        }
        None => DEFAULT_TIMEOUT_MS,
    };
    let timeout = Duration::from_millis(timeout_ms as u64);

    match round_trip(&host_port, &app, &to_json(payload), timeout) {
        Ok(answer) => from_json(out, &answer),
        Err(message) => exception(out, "net_client", &message),
    }
}

/// `http://host:port/app` → `("host:port", "app")`.
///
/// The app segment is optional: `http://host:port` is a program that serves
/// only itself, and the far side gets an empty `app`.
fn split_url(url: &str) -> Result<(String, String), String> {
    let rest = url
        .strip_prefix(SCHEME)
        .ok_or_else(|| match url.strip_prefix("https://") {
            // Worth its own sentence: this module speaks no TLS, and a url
            // that says `https` and is then sent in the clear would be the
            // worst kind of working.
            Some(_) => format!(
                "`https://` needs TLS, which this module does not speak — put a proxy in \
                 front and send it `{SCHEME}`, got '{url}'"
            ),
            None => format!("url must start with `{SCHEME}` — got '{url}'"),
        })?;
    let (host_port, app) = match rest.split_once('/') {
        Some((host_port, app)) => (host_port, app),
        None => (rest, ""),
    };
    if host_port.is_empty() {
        return Err(format!("url has no host — got '{url}'"));
    }
    if !host_port.contains(':') {
        return Err(format!(
            "url needs a port, as `{SCHEME}host:port/app` — got '{url}'"
        ));
    }
    if app.contains('/') {
        return Err(format!(
            "url has more than an app segment — this transport has no paths, got '{url}'"
        ));
    }
    Ok((host_port.to_string(), app.to_string()))
}

/// Connect, POST the particle, read the answer, close.
///
/// No connection reuse: a particle is one exchange, and a pool would be state
/// this module would then have to invalidate. `net_server` closes its side
/// after answering for the same reason, and says so with `connection: close`.
///
/// Written by hand rather than reaching for an HTTP client: what is needed is
/// one request with one header and a body, and a response whose only
/// interesting header is its length.
fn round_trip(
    host_port: &str,
    app: &str,
    particle: &Json,
    timeout: Duration,
) -> Result<Json, String> {
    let addr = host_port
        .to_socket_addrs()
        .map_err(|e| format!("cannot resolve '{host_port}': {e}"))?
        .next()
        .ok_or_else(|| format!("'{host_port}' resolved to no address"))?;

    let mut stream = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| format!("cannot reach '{host_port}': {e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|_| stream.set_write_timeout(Some(timeout)))
        .map_err(|e| format!("cannot set a timeout on '{host_port}': {e}"))?;

    // The body is the particle, and the path is the app. Nothing wraps
    // anything, which is also what makes this a request anyone can send:
    // `curl -d '{"_class":"Ping"}' http://host:port/app` is the whole of it.
    let body = serde_json::to_vec(particle).map_err(|e| format!("cannot encode: {e}"))?;
    let head = format!(
        "POST /{app} HTTP/1.1\r\n\
         host: {host_port}\r\n\
         content-type: application/json\r\n\
         content-length: {}\r\n\
         connection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(&body))
        .and_then(|_| stream.flush())
        .map_err(|e| format!("cannot send to '{host_port}': {e}"))?;

    // Read it all: the far side closes after answering, so end-of-stream is
    // the end of the response and there is no length to trust.
    let mut answer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                answer.extend_from_slice(&chunk[..n]);
                if answer.len() > MAX_ANSWER_BYTES {
                    return Err(format!(
                        "'{host_port}' answered with over the {MAX_ANSWER_BYTES}-byte cap"
                    ));
                }
            }
            Err(e) => return Err(format!("no answer from '{host_port}': {e}")),
        }
    }

    let split = answer
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| format!("'{host_port}' answered without a complete head"))?;
    let body = &answer[split + 4..];
    if body.is_empty() {
        // A status line worth repeating: this is what a sender sees when it
        // reached something that is not a `net_server` at all.
        let status = String::from_utf8_lossy(&answer[..split])
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        return Err(format!("'{host_port}' answered with no body ({status})"));
    }

    serde_json::from_slice(body)
        .map_err(|e| format!("answer from '{host_port}' is not JSON: {e}"))
}

// ---------------------------------------------------------------------------
// CodeValue <-> JSON
// ---------------------------------------------------------------------------

/// A code value to its JSON form, **`_class` included** — it is what decides
/// which handler runs on the far side. (The `json` module's `Stringify` drops
/// it, because there a caller asking for JSON wants their data, not the
/// language's bookkeeping.)
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
                map.insert(key.to_owned(), to_json(value));
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
/// as an integer; everything else is a float. A non-finite value has no JSON
/// spelling, so it becomes `null` rather than failing the whole frame.
fn number_to_json(n: f64) -> Json {
    if !n.is_finite() {
        Json::Null
    } else if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
        Json::Number((n as i64).into())
    } else {
        serde_json::Number::from_f64(n).map_or(Json::Null, Json::Number)
    }
}

//! The `net` native module — HTTP(S) requests for the Code programming
//! language, written in Rust on [`code-native`].
//!
//! Handlers (see `README.md` for the API contract and the reasoning):
//!
//! - `Get`  — `{ url, headers?, timeout_seconds?, max_body_bytes? }`
//! - `Post` — the same plus `{ body, content_type? }`
//!
//! Both answer with `HttpResponse { ok, status, body }`.
//!
//! The one rule that shapes everything here: **a request that fails is a
//! value, not an error.** This language has no `try`/`catch` and no
//! catchable assert, so calling `runtime_error` on a refused connection
//! would end the program with nothing able to recover. `runtime_error` is
//! reserved for misuse the program could have avoided — a missing `url`, a
//! header value that isn't a String — which is a bug, and aborts like every
//! other module's bugs do.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use std::time::Duration;

use code_native::*;

/// Whole-request budget when the particle doesn't say. Short enough that a
/// language with no way to interrupt itself doesn't sit there looking hung.
const DEFAULT_TIMEOUT_SECONDS: f64 = 10.0;
/// Most response bytes accepted when the particle doesn't say — 1 MiB. A
/// cap rather than a knob-with-no-default: forgetting the field should not
/// let a download run away with the process. Exceeding it fails the request
/// rather than truncating — see `README.md`.
const DEFAULT_MAX_BODY_BYTES: f64 = 1_048_576.0;

// The optional inbound export: `net` speaks first, to report what went
// wrong, rather than only answering. A program that defines no `Exception`
// or `Log` handler simply never hears it — a pushed class nothing handles is
// dropped (decided 2026-08-28), which is exactly what makes diagnostics safe
// to send unasked.
code_native::declare_inbound!();

/// Push `Exception { source, message }` into the program. Best effort in
/// both directions: the host may never have taken an inbound channel, and
/// the program may have no handler — neither is this module's problem, and
/// neither changes what `Get`/`Post` return.
fn report_exception(message: &str) {
    let mut particle = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(3);
    borrowed_str(buf.slot_mut(0), c"Exception");
    borrowed_str(buf.slot_mut(1), c"net");
    owned_str(buf.slot_mut(2), message);
    object(&mut particle, &[c"_class", c"source", c"message"], &mut buf);
    buf.release_all();
    emit_inbound(&particle);
    release(&mut particle);
}

/// Push `Log { source, level, message }` into the program. Same field names
/// and levels as the `euglena-language` organelles use, so a handler written
/// against those reads the same here.
fn report_log(level: &str, message: &str) {
    let mut particle = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(4);
    borrowed_str(buf.slot_mut(0), c"Log");
    borrowed_str(buf.slot_mut(1), c"net");
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

/// The ABI version this module speaks. Must equal `CODE_ABI_VERSION` or the
/// host refuses to load us.
#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// The single dispatch point: read the particle's `_class`, route to the
/// matching handler. Same shape as `strings` and `math`, so a mis-emitted
/// particle points at itself in both backends.
///
/// # Safety
///
/// Both pointers must be valid for reads/writes respectively for the
/// duration of the call, and refer to values laid out per `code_abi.h` —
/// the host guarantees this on every dispatch (see `native.rs`).
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    if particle.tag != CodeTag::Object {
        runtime_error("net: emit requires a particle");
    }
    let class = match read_field_str(particle, "_class") {
        Some(c) => c,
        None => runtime_error("net: emit requires a particle"),
    };
    let out = &mut *out;

    match class {
        "Get" => request(out, particle, Method::Get),
        "Post" => request(out, particle, Method::Post),
        other => runtime_error(&format!("net: unknown handler '{other}'")),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Method {
    Get,
    Post,
}

impl Method {
    fn class(self) -> &'static str {
        match self {
            Method::Get => "Get",
            Method::Post => "Post",
        }
    }
}

// ---------------------------------------------------------------------------
// Operand extraction — a missing field and a wrong-typed field are different
// mistakes, and the errors say which (the convention `strings` set).
// ---------------------------------------------------------------------------

fn require_url<'a>(particle: &'a CodeValue, class: &str) -> &'a str {
    let field = match find_field(particle, "url") {
        Some(v) => v,
        None => runtime_error(&format!("net: {class} requires a 'url' field")),
    };
    match read_str(field) {
        Some("") => runtime_error(&format!("net: {class} requires a non-empty 'url'")),
        Some(s) => s,
        None => runtime_error(&format!("net: {class} requires a string 'url'")),
    }
}

/// An optional Number field, floored at zero and refused if present with the
/// wrong type — absent means "use the default", but present-and-a-String is
/// a mistake worth naming rather than silently defaulting over.
fn optional_number(particle: &CodeValue, class: &str, name: &str, default: f64) -> f64 {
    match find_field(particle, name) {
        None => default,
        Some(v) => match read_number(v) {
            Some(n) if n > 0.0 => n,
            Some(_) => runtime_error(&format!("net: {class} requires a positive '{name}'")),
            None => runtime_error(&format!("net: {class} requires a number '{name}'")),
        },
    }
}

/// `headers` as `(name, value)` pairs. Absent is an empty list; present but
/// not an Object, or holding a non-String value, is misuse.
///
/// Walks `keys`/`items` directly — `code-native` exposes `array_elems` for
/// arrays but no equivalent pairs iterator for objects, and the layout is
/// public and documented (`keys` is parallel to `items`, both `len` long).
fn headers(particle: &CodeValue, class: &str) -> Vec<(String, String)> {
    let Some(field) = find_field(particle, "headers") else {
        return Vec::new();
    };
    if field.tag != CodeTag::Object || field.keys.is_null() {
        runtime_error(&format!("net: {class} requires an object 'headers'"));
    }
    let mut out = Vec::new();
    for i in 0..field.len {
        let key = unsafe { *field.keys.offset(i as isize) };
        if key.is_null() {
            continue;
        }
        let name = unsafe { std::ffi::CStr::from_ptr(key) }
            .to_str()
            .unwrap_or_default();
        // `_class` is an ordinary field on every particle, so an object
        // literal used as headers may carry one. It is never a header.
        if name.is_empty() || name == "_class" {
            continue;
        }
        let value = unsafe { &*slot_at(field.items, i) };
        match read_str(value) {
            Some(v) => out.push((name.to_string(), v.to_string())),
            None => runtime_error(&format!(
                "net: {class} requires string header values, but '{name}' is not a string"
            )),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The request itself
// ---------------------------------------------------------------------------

fn request(out: &mut CodeValue, particle: &CodeValue, method: Method) {
    let class = method.class();
    let url = require_url(particle, class);
    let timeout = optional_number(particle, class, "timeout_seconds", DEFAULT_TIMEOUT_SECONDS);
    let max_body =
        optional_number(particle, class, "max_body_bytes", DEFAULT_MAX_BODY_BYTES) as u64;
    let headers = headers(particle, class);

    // Both are `Post`-only, and read only for `Post`: checking them on a
    // `Get` would reject a field that handler does not have, with a message
    // naming a handler the program did not emit.
    let (body, content_type) = match method {
        Method::Get => (String::new(), String::new()),
        Method::Post => {
            let body = match find_field(particle, "body") {
                Some(v) => match read_str(v) {
                    Some(s) => s.to_string(),
                    None => runtime_error("net: Post requires a string 'body'"),
                },
                None => runtime_error("net: Post requires a 'body' field"),
            };
            let content_type = match find_field(particle, "content_type") {
                None => "application/octet-stream".to_string(),
                Some(v) => match read_str(v) {
                    Some(s) => s.to_string(),
                    None => runtime_error("net: Post requires a string 'content_type'"),
                },
            };
            (body, content_type)
        }
    };

    match perform(
        method,
        url,
        &headers,
        &body,
        &content_type,
        timeout,
        max_body,
    ) {
        Ok((status, body)) => {
            // `Info` for a request that completed, whatever the server
            // thought of it — a 404 is news, not a fault.
            report_log("Info", &format!("{class} {url} -> {}", status as i64));
            response(out, true, status, &body)
        }
        // Everything ureq can fail with lands here as `ok: false` — refused,
        // unresolvable, timed out, malformed URL, TLS rejected. The message
        // rides along in `body` so a program can print it; `status` is 0
        // because there was no HTTP response to have a status.
        //
        // The `Exception` push is *additional* to that, never instead of it:
        // a program that ignores diagnostics still gets the whole story from
        // the value it was handed, which is what keeps checking `ok` a
        // complete way to use this module.
        Err(message) => {
            report_exception(&format!("{class} {url}: {message}"));
            response(out, false, 0.0, &message)
        }
    }
}

/// The one place that talks to the network. Returns `Err(message)` for
/// anything that stopped a response arriving; an HTTP error *status* is a
/// perfectly good response and comes back as `Ok`.
fn perform(
    method: Method,
    url: &str,
    headers: &[(String, String)],
    body: &str,
    content_type: &str,
    timeout_seconds: f64,
    max_body_bytes: u64,
) -> Result<(f64, String), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs_f64(timeout_seconds)))
        // 4xx/5xx must reach us as a response, not an error — the status is
        // exactly what the caller asked for.
        .http_status_as_error(false)
        .build()
        .into();

    // The two branches cannot share a variable: ureq types its builder by
    // whether a body is coming (`WithoutBody`/`WithBody`), so they only meet
    // again at the `Result<Response, _>` both sends produce.
    let mut response = match method {
        Method::Get => with_headers(agent.get(url), headers).call(),
        Method::Post => with_headers(
            agent.post(url).header("Content-Type", content_type),
            headers,
        )
        .send(body),
    }
    .map_err(|e| e.to_string())?;

    let status = response.status().as_u16() as f64;
    let text = response
        .body_mut()
        .with_config()
        // `+ 1` is not slack. ureq's `LimitReader` raises the error when its
        // budget is *already* zero and another read arrives, and
        // `read_to_string` always reads once more to see EOF — so `limit(n)`
        // admits n-1 bytes. One more makes `max_body_bytes` mean exactly
        // "this many bytes are fine".
        .limit(max_body_bytes.saturating_add(1))
        .read_to_string()
        .map_err(|e| match e {
            // Worth its own wording twice over: ureq calls this a "request
            // limit", which reads as though the *request* was too big, and
            // it names the internal budget (`max + 1`) rather than the
            // number the program actually wrote.
            ureq::Error::BodyExceedsLimit(_) => {
                format!("response body exceeds max_body_bytes ({max_body_bytes})")
            }
            other => other.to_string(),
        })?;

    Ok((status, text))
}

/// Apply caller-supplied headers, generic over ureq's body-presence type
/// parameter so `Get` and `Post` share one implementation.
fn with_headers<T>(
    mut builder: ureq::RequestBuilder<T>,
    headers: &[(String, String)],
) -> ureq::RequestBuilder<T> {
    for (name, value) in headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    builder
}

/// Build `HttpResponse { ok, status, body }`.
///
/// Not `make_result`: that helper builds the `{ _class, value }` shape core
/// handlers use, and this response has three fields rather than one.
/// `emit … get r` binds whatever object comes back, so `r.status` works
/// either way (see `native.rs`'s dispatch, which does no unwrapping).
fn response(out: &mut CodeValue, ok: bool, status: f64, body: &str) {
    let mut buf = SlotBuffer::new(4);
    borrowed_str(buf.slot_mut(0), c"HttpResponse");
    boolean(buf.slot_mut(1), ok);
    number(buf.slot_mut(2), status);
    owned_str(buf.slot_mut(3), body);
    object(out, &[c"_class", c"ok", c"status", c"body"], &mut buf);
    buf.release_all();
}

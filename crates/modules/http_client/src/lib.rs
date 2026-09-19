//! The `http_client` native module — HTTP(S) requests for the Code
//! programming language, written in Rust on [`code-native`].
//!
//! Handlers (see `README.md` for the API contract and the reasoning). One
//! particle per method, because dispatch in this language is already a
//! `_class` switch and a `method` field would make this module run a second
//! switch on a string, re-implementing the dispatcher one level down:
//!
//! - `Get`, `Delete`, `Head`, `Options` —
//!   `{ url, headers?, timeout_seconds?, max_body_bytes? }`
//! - `Post`, `Put`, `Patch` — the same plus `{ body, content_type? }`
//!
//! All seven answer with `HttpResponse { ok, status, body }`. A `Head`
//! response carries no body by definition, so its `body` is the empty
//! string — that is HTTP's answer, not a special case here.
//!
//! The one rule that shapes everything here: **a request that fails is a
//! value, not an error.** As of 2026-08-28 that is the whole rule — there is
//! no second category. A module may never end the application
//! (`docs/todo/errors-as-particles.md`), so there is nothing this module can
//! do but answer.
//!
//! In practice that means **no validation pass**. A field the particle does
//! not carry is null, exactly as `.field` reads an absent member, and a null
//! url is just a url that cannot be fetched — so the failure comes from
//! *attempting* the request rather than from a guard refusing to try. The
//! messages are better for it: `bad uri: 42 is missing scheme` says more
//! than a hand-written "requires a string 'url'" did.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use code_native::*;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

/// Whole-request budget when the particle doesn't say. Short enough that a
/// language with no way to interrupt itself doesn't sit there looking hung.
const DEFAULT_TIMEOUT_SECONDS: f64 = 10.0;
/// Most response bytes accepted when the particle doesn't say — 1 MiB. A
/// cap rather than a knob-with-no-default: forgetting the field should not
/// let a download run away with the process. Exceeding it fails the request
/// rather than truncating — see `README.md`.
const DEFAULT_MAX_BODY_BYTES: f64 = 1_048_576.0;

/// The ordinary client remains backwards compatible. The catalog and worker
/// opt into `public_https`, which makes the resolver and agent enforce the
/// public-service boundary at the socket rather than trusting URL text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NetworkPolicy {
    Default,
    PublicHttps,
}

impl NetworkPolicy {
    fn from_particle(particle: &CodeValue) -> Result<Self, String> {
        match read_field_str(particle, "network_policy") {
            None => Ok(Self::Default),
            Some("public_https") => Ok(Self::PublicHttps),
            Some(other) => Err(format!(
                "unknown network_policy '{other}' (expected 'public_https')"
            )),
        }
    }
}

/// Resolver used by the service catalog and other trusted crawlers. ureq uses
/// the addresses returned here for the actual connector, so filtering this
/// list closes the common resolve-then-connect gap instead of merely checking
/// the hostname before handing it back to ureq.
#[derive(Debug, Default)]
struct PublicHttpsResolver;

impl Resolver for PublicHttpsResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        if uri.scheme_str() != Some("https") {
            return Err(ureq::Error::RequireHttpsOnly(uri.to_string()));
        }

        let authority = uri.authority().ok_or_else(|| {
            ureq::Error::BadUri("public_https URL has no authority".to_string())
        })?;
        if authority.as_str().contains('@') {
            return Err(ureq::Error::BadUri(
                "public_https URL must not contain user-info".to_string(),
            ));
        }

        let port = uri.port_u16().unwrap_or(443);
        if port != 443 {
            return Err(ureq::Error::BadUri(format!(
                "public_https URL must use port 443, got {port}"
            )));
        }

        let resolved = DefaultResolver::default().resolve(uri, config, timeout)?;
        let mut allowed = self.empty();
        for address in &resolved[..] {
            if is_public_ip(address.ip()) {
                allowed.push(*address);
            }
        }

        if allowed.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(allowed)
    }
}

/// Public service destinations exclude addresses that can reach this machine,
/// private networks, documentation/test ranges and special-purpose address
/// space. The check is applied to every resolved address; a hostname is safe
/// only when all addresses ureq may try are safe.
fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(value) => is_public_ipv4(value),
        IpAddr::V6(value) => is_public_ipv6(value),
    }
}

fn is_public_ipv4(value: Ipv4Addr) -> bool {
    let [first, second, third, _] = value.octets();

    if first == 0 || first >= 224 || first == 127 {
        return false;
    }
    if first == 10
        || (first == 172 && (16..=31).contains(&second))
        || (first == 192 && second == 168)
    {
        return false;
    }
    if first == 169 && second == 254 {
        return false;
    }
    if first == 100 && (64..=127).contains(&second) {
        return false;
    }
    if first == 192 && second == 0 && third == 0 {
        return false;
    }
    if first == 192 && second == 0 && third == 2 {
        return false;
    }
    if first == 198 && (second == 18 || second == 19 || second == 51 && third == 100) {
        return false;
    }
    if first == 203 && second == 0 && third == 113 {
        return false;
    }

    true
}

fn is_public_ipv6(value: Ipv6Addr) -> bool {
    if value.is_unspecified() || value.is_loopback() || value.is_multicast() {
        return false;
    }

    let segments = value.segments();
    // fc00::/7 unique-local and fe80::/10 link-local.
    if segments[0] & 0xfe00 == 0xfc00 || segments[0] & 0xffc0 == 0xfe80 {
        return false;
    }
    // Documentation, benchmarking, discard-only and NAT64 well-known ranges.
    if (segments[0] == 0x2001 && segments[1] == 0x0db8)
        || (segments[0] == 0x2001 && segments[1] == 0x0002)
        || (segments[0] == 0x0100 && segments[1..].iter().all(|part| *part == 0))
        || (segments[0] == 0x0064
            && segments[1] == 0xff9b
            && segments[2..].iter().all(|part| *part == 0))
    {
        return false;
    }

    // Reject private IPv4 space hidden inside IPv4-mapped IPv6 addresses.
    if segments[..6] == [0, 0, 0, 0, 0, 0xffff] {
        let embedded = Ipv4Addr::new(
            (segments[6] >> 8) as u8,
            segments[6] as u8,
            (segments[7] >> 8) as u8,
            segments[7] as u8,
        );
        return is_public_ipv4(embedded);
    }

    // IPv4-compatible addresses are deprecated but can still hide a local
    // destination in the final 32 bits. Treat them with the same rule.
    if segments[..6] == [0, 0, 0, 0, 0, 0] {
        let embedded = Ipv4Addr::new(
            (segments[6] >> 8) as u8,
            segments[6] as u8,
            (segments[7] >> 8) as u8,
            segments[7] as u8,
        );
        return is_public_ipv4(embedded);
    }

    true
}

// The optional inbound export: this module speaks first, to report what went
// wrong, rather than only answering. A program that defines no `Exception`
// or `Log` handler simply never hears it — a pushed class nothing handles is
// dropped (decided 2026-08-28), which is exactly what makes diagnostics safe
// to send unasked.
code_native::declare_inbound!();
code_native::declare_inbound_reply!(answered);

/// The program answered a response pushed with `later`; nothing waits on it.
fn answered(_particle: &CodeValue, _result: &CodeValue) {}

/// One number per `later` request, so an answer finds its ask.
static NEXT_REQUEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
use std::sync::atomic::Ordering;

/// Push `Exception { source, message }` into the program. Best effort in
/// both directions: the host may never have taken an inbound channel, and
/// the program may have no handler — neither is this module's problem, and
/// neither changes what `Get`/`Post` return.
fn report_exception(message: &str) {
    let mut particle = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(3);
    borrowed_str(buf.slot_mut(0), c"Exception");
    borrowed_str(buf.slot_mut(1), c"http_client");
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
    borrowed_str(buf.slot_mut(1), c"http_client");
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
    // Everything below runs inside `guarded`, so a panic anywhere in this
    // module — ours or one of ureq's — becomes an `Exception` rather than
    // taking the host down with it.
    guarded(&mut *out, "http_client", |out| {
        // Not a particle, or a class this module does not handle: null.
        // Neither is an error — whether to act on a particle is the
        // recipient's business (docs/todo/errors-as-particles.md).
        match read_field_str(particle, "_class") {
            Some("Get") => request(out, particle, Method::Get),
            Some("Post") => request(out, particle, Method::Post),
            Some("Put") => request(out, particle, Method::Put),
            Some("Patch") => request(out, particle, Method::Patch),
            Some("Delete") => request(out, particle, Method::Delete),
            Some("Head") => request(out, particle, Method::Head),
            Some("Options") => request(out, particle, Method::Options),
            _ => null(out),
        }
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl Method {
    fn class(self) -> &'static str {
        match self {
            Method::Get => "Get",
            Method::Post => "Post",
            Method::Put => "Put",
            Method::Patch => "Patch",
            Method::Delete => "Delete",
            Method::Head => "Head",
            Method::Options => "Options",
        }
    }

    /// Whether this method carries a request body — which is ureq's own
    /// split (`RequestBuilder<WithBody>` against `WithoutBody`) and the only
    /// thing that makes the seven methods anything but one list.
    ///
    /// `Delete` is on the bodyless side. HTTP permits a body on it and
    /// forbids nothing, but no defined semantics attach to one, and servers
    /// disagree about whether it even arrives; a module that offered the
    /// field would be promising something it cannot deliver.
    fn has_body(self) -> bool {
        matches!(self, Method::Post | Method::Put | Method::Patch)
    }
}

// ---------------------------------------------------------------------------
// Operand extraction.
//
// There is deliberately no validation pass here. A field the particle does
// not carry is null, exactly as `.field` reads an absent member, and a null
// url is simply a url that cannot be fetched — so the failure comes from
// *attempting* the request rather than from a guard refusing to try. That
// keeps this module out of the business of judging its caller, and the
// message honest: `ureq` says `bad uri: unknown scheme` far better than a
// hand-written "requires a string 'url'" ever did.
// ---------------------------------------------------------------------------

/// The url as text, whatever kind the field turned out to be. A missing or
/// non-String field renders as the empty string, which is a url the request
/// stage rejects on its own.
fn url_text(particle: &CodeValue) -> String {
    match find_field(particle, "url") {
        Some(v) => value_text(v),
        None => String::new(),
    }
}

/// A value as text for the one purpose this module has: naming it in a url
/// or a header. Non-strings render as themselves rather than as "", so a
/// `url = 42` reports `bad uri: '42'` instead of a misleading "no url".
fn value_text(v: &CodeValue) -> String {
    match read_str(v) {
        Some(s) => s.to_string(),
        None => match read_number(v) {
            // Integral numbers without a trailing `.0`, matching how the
            // language itself renders them.
            Some(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", n as i64),
            Some(n) => format!("{n}"),
            None => match read_bool(v) {
                Some(b) => b.to_string(),
                None => String::new(),
            },
        },
    }
}

/// An optional positive Number. Anything else — absent, the wrong kind, zero
/// or negative — falls back to the default rather than refusing.
///
/// The fallback is not politeness: a non-positive duration would panic
/// inside `Duration::from_secs_f64`, and `guarded` would turn that into an
/// `Exception` naming a Rust internal rather than anything the caller could
/// act on. Defaulting says the same thing more usefully.
fn optional_number(particle: &CodeValue, name: &str, default: f64) -> f64 {
    match find_field(particle, name).and_then(read_number) {
        Some(n) if n > 0.0 && n.is_finite() => n,
        _ => default,
    }
}

/// `headers` as `(name, value)` pairs. Absent is an empty list; present but
/// not an Object, or holding a non-String value, is misuse.
///
/// Walks `keys`/`items` directly — `code-native` exposes `array_elems` for
/// arrays but no equivalent pairs iterator for objects, and the layout is
/// public and documented (`keys` is parallel to `items`, both `len` long).
fn headers(
    particle: &CodeValue,
    network_policy: NetworkPolicy,
) -> Result<Vec<(String, String)>, String> {
    let Some(field) = find_field(particle, "headers") else {
        return Ok(Vec::new());
    };
    // Not an object: no headers. Nothing to refuse — a caller that meant to
    // send some and did not will see that in the request that arrives.
    if field.tag != CodeTag::Object || field.keys.is_null() {
        return Ok(Vec::new());
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
        if network_policy == NetworkPolicy::PublicHttps
            && public_header_is_forbidden(name)
        {
            return Err(format!(
                "network_policy public_https rejects header '{name}'"
            ));
        }
        // Rendered rather than required to be a String, for the same reason
        // the url is: `"X-Count" = 3` sends `3`, which is what the caller
        // plainly meant.
        let value = value_text(unsafe { &*slot_at(field.items, i) });
        if !value.is_empty() {
            out.push((name.to_string(), value));
        }
    }
    Ok(out)
}

/// Headers that must not cross the public-service boundary from an arbitrary
/// caller. ureq owns framing and the transport hop headers; authorization and
/// cookies are especially easy to accidentally turn into credential exfiltration
/// when a catalog URL is user-controlled.
fn public_header_is_forbidden(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "cookie"
            | "connection"
            | "content-length"
            | "expect"
            | "keep-alive"
            | "host"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

// ---------------------------------------------------------------------------
// The request itself
// ---------------------------------------------------------------------------

fn request(out: &mut CodeValue, particle: &CodeValue, method: Method) {
    let class = method.class();
    let url = url_text(particle);
    let network_policy = match NetworkPolicy::from_particle(particle) {
        Ok(policy) => policy,
        Err(message) => {
            report_exception(&format!("{class} {url}: {message}"));
            response(out, false, 0.0, &message);
            return;
        }
    };
    let timeout = optional_number(particle, "timeout_seconds", DEFAULT_TIMEOUT_SECONDS);
    let max_body = optional_number(particle, "max_body_bytes", DEFAULT_MAX_BODY_BYTES) as u64;
    let headers = match headers(particle, network_policy) {
        Ok(headers) => headers,
        Err(message) => {
            report_exception(&format!("{class} {url}: {message}"));
            response(out, false, 0.0, &message);
            return;
        }
    };

    // Body-carrying methods only. An absent body is an empty one, which is a
    // legal request, and an absent content type takes the default.
    let (body, content_type) = if method.has_body() {
        let body = find_field(particle, "body")
            .map(value_text)
            .unwrap_or_default();
        let content_type = match find_field(particle, "content_type").map(value_text) {
            Some(ct) if !ct.is_empty() => ct,
            _ => "application/octet-stream".to_string(),
        };
        (body, content_type)
    } else {
        (String::new(), String::new())
    };

    // `later = true`: the request goes out on a thread of its own, this
    // answers `Sent { id }` at once, and the same `HttpResponse` arrives
    // afterwards as a particle carrying `_request_id = id` — the shape
    // `net_client` already has in a browser. Nothing else about the request
    // changes; a program that does not ask for it never sees it.
    if read_field_bool(particle, "later") == Some(true) {
        let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::SeqCst);
        let class = class.to_string();
        std::thread::spawn(move || {
            let outcome = perform(
                method,
                &url,
                &headers,
                &body,
                &content_type,
                timeout,
                max_body,
                network_policy,
            );
            let (ok, status, text) = match outcome {
                Ok((status, body)) => {
                    report_log(
                        "Info",
                        &format!("{class} {url} -> {} (later, {id})", status as i64),
                    );
                    (true, status, body)
                }
                Err(message) => {
                    report_exception(&format!("{class} {url}: {message} (later, {id})"));
                    (false, 0.0, message)
                }
            };
            let mut buf = SlotBuffer::new(5);
            borrowed_str(buf.slot_mut(0), c"HttpResponse");
            boolean(buf.slot_mut(1), ok);
            number(buf.slot_mut(2), status);
            owned_str(buf.slot_mut(3), &text);
            number(buf.slot_mut(4), id as f64);
            let mut particle = CodeValue::zeroed();
            object(
                &mut particle,
                &[c"_class", c"ok", c"status", c"body", c"_request_id"],
                &mut buf,
            );
            buf.release_all();
            emit_inbound(&particle);
            release(&mut particle);
        });
        make_result(out, c"Sent", |slot| number(slot, id as f64));
        return;
    }

    match perform(
        method,
        &url,
        &headers,
        &body,
        &content_type,
        timeout,
        max_body,
        network_policy,
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
    network_policy: NetworkPolicy,
) -> Result<(f64, String), String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs_f64(timeout_seconds)))
        // 4xx/5xx must reach us as a response, not an error — the status is
        // exactly what the caller asked for.
        .http_status_as_error(false);
    let agent: ureq::Agent = match network_policy {
        NetworkPolicy::Default => config.build().into(),
        NetworkPolicy::PublicHttps => {
            // Do not let an environment proxy resolve an untrusted target on
            // our behalf. The resolver must see and filter the target address.
            // Redirects are returned to the caller instead of being followed;
            // a catalog revision may explicitly model a second origin later.
            let config = config
                .https_only(true)
                .max_redirects(0)
                .max_redirects_will_error(false)
                .proxy(None)
                .build();
            ureq::Agent::with_parts(config, DefaultConnector::new(), PublicHttpsResolver)
        }
    };

    // The two groups cannot share a variable: ureq types its builder by
    // whether a body is coming (`WithoutBody`/`WithBody`), so they only meet
    // again at the `Result<Response, _>` both sends produce.
    let mut response = match method {
        Method::Get => with_headers(agent.get(url), headers).call(),
        Method::Delete => with_headers(agent.delete(url), headers).call(),
        Method::Head => with_headers(agent.head(url), headers).call(),
        Method::Options => with_headers(agent.options(url), headers).call(),
        Method::Post => with_headers(
            agent.post(url).header("Content-Type", content_type),
            headers,
        )
        .send(body),
        Method::Put => {
            with_headers(agent.put(url).header("Content-Type", content_type), headers)
                .send(body)
        }
        Method::Patch => with_headers(
            agent.patch(url).header("Content-Type", content_type),
            headers,
        )
        .send(body),
    }
    .map_err(|e| e.to_string())?;

    let status_code = response.status().as_u16();
    if network_policy == NetworkPolicy::PublicHttps && (300..400).contains(&status_code) {
        return Err(format!(
            "network_policy public_https does not follow redirect status {status_code}"
        ));
    }
    let status = status_code as f64;
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
/// parameter so all seven methods share one implementation.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_ipv4_policy_rejects_special_ranges() {
        for address in [
            "0.0.0.0",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.1.1",
            "172.16.0.1",
            "192.0.0.1",
            "192.0.2.1",
            "192.168.1.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "240.0.0.1",
        ] {
            let ip = address.parse().expect("parse IPv4 fixture");
            assert!(!is_public_ip(IpAddr::V4(ip)), "accepted {address}");
        }

        let public = "1.1.1.1".parse().expect("parse public IPv4 fixture");
        assert!(is_public_ip(IpAddr::V4(public)));
    }

    #[test]
    fn public_ipv6_policy_rejects_local_documentation_and_mapped_private() {
        for address in [
            "::",
            "::1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "2001:2::1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.1.1",
            "::192.0.2.1",
        ] {
            let ip = address.parse().expect("parse IPv6 fixture");
            assert!(!is_public_ip(IpAddr::V6(ip)), "accepted {address}");
        }

        let public = "2606:4700:4700::1111"
            .parse()
            .expect("parse public IPv6 fixture");
        assert!(is_public_ip(IpAddr::V6(public)));
    }

    #[test]
    fn public_https_policy_rejects_credential_and_transport_headers() {
        for header in [
            "Authorization",
            "Cookie",
            "Connection",
            "Host",
            "Proxy-Authorization",
            "Transfer-Encoding",
            "Upgrade",
        ] {
            assert!(public_header_is_forbidden(header), "accepted {header}");
        }
        assert!(!public_header_is_forbidden("Accept"));
        assert!(!public_header_is_forbidden("Content-Type"));
    }

    #[test]
    fn policy_parser_keeps_default_client_unchanged() {
        let particle = CodeValue::zeroed();
        assert_eq!(
            NetworkPolicy::from_particle(&particle),
            Ok(NetworkPolicy::Default)
        );
    }
}
}

#[cfg(target_arch = "wasm32")]
mod page {
    include!("../../browser_half.rs");
    browser_half!(
        "http_client",
        http_client_code_module_abi_version,
        http_client_code_module_dispatch
    );
}

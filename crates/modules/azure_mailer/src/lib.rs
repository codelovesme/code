//! The `azure_mailer` native module — send email through Azure Communication
//! Services, for the Code programming language, written in Rust on
//! [`code-native`].
//!
//! [`mailer`](../mailer) speaks SMTP, which every provider understands. This
//! speaks one provider's REST API, and exists because that provider's
//! credential is a *connection string* rather than a mailbox and a password:
//! an account can send without a password for a human mailbox existing at
//! all.
//!
//! **`Send` is `mailer`'s, field for field**, so moving between them is one
//! word in a manifest and not a line of any gene — the same promise
//! `membrane` makes to `net_server`. Only `Config` differs, because the two
//! are configured by genuinely different things.
//!
//! - `Config { connection_string, from }` → `ConfigResult { ok }` — the setup
//!   particle. `connection_string` is the one Azure hands out,
//!   `endpoint=https://….communication.azure.com/;accesskey=…`; `from` is a
//!   sender on a domain linked to that resource.
//! - `Send { recipient, subject?, text?, html?, from?, cc?, bcc? }` →
//!   `SendResult { ok }`.
//!
//! Requests are signed with Azure's HMAC-SHA256 scheme: the access key signs
//! the method, the path, the date, the host and a hash of the body, so a
//! request cannot be replayed against a different body or a different route.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use code_native::*;
use hmac::{Hmac, Mac};
use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

static CONFIG: Mutex<Option<Config>> = Mutex::new(None);

struct Config {
    /// No trailing slash, so joining a path never doubles one.
    endpoint: String,
    host: String,
    access_key: Vec<u8>,
    from: String,
}

/// The API version this module was written against. Pinned rather than
/// floating: a provider changing its contract under a running program is the
/// failure this avoids.
const API_VERSION: &str = "2023-03-31";

const NOT_CONFIGURED: &str =
    "azure_mailer has no transport — send Config { connection_string, from } first";

/// The ABI version this module speaks. Must equal `CODE_ABI_VERSION` or the
/// host refuses to load us.
#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// The single dispatch point: read `_class`, route to a handler. An
/// unhandled class is null; a handler that cannot do the work returns an
/// `Exception`. Neither ends the program.
///
/// # Safety
///
/// Both pointers must be valid for reads/writes for the duration of the
/// call and laid out per `code_abi.h` — the host guarantees this.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "azure_mailer", |out| {
        let class = read_field_str(particle, "_class").unwrap_or("");
        let outcome = match class {
            "Config" => config(out, particle),
            "Send" => send(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "azure_mailer", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let connection_string = require_str(particle, "connection_string", "Config")?;
    let from = require_str(particle, "from", "Config")?.to_string();

    let (endpoint, access_key) = parse_connection_string(connection_string)?;
    let host = host_of(&endpoint)?;

    *CONFIG.lock().unwrap_or_else(|e| e.into_inner()) = Some(Config {
        endpoint,
        host,
        access_key,
        from,
    });

    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"ConfigResult");
    boolean(b.slot_mut(1), true);
    object(out, &[c"_class", c"ok"], &mut b);
    b.release_all();
    Ok(())
}

/// `endpoint=https://x.communication.azure.com/;accesskey=<base64>`, in either
/// order and with whatever spacing. The key is base64 in the string and raw
/// bytes once decoded — signing with the text would produce a signature the
/// service rejects, with nothing to say why.
fn parse_connection_string(s: &str) -> Result<(String, Vec<u8>), String> {
    let mut endpoint = None;
    let mut access_key = None;

    for part in s.split(';') {
        let part = part.trim();
        // Azure writes the keys lowercase; accept any case rather than fail
        // on a string somebody retyped.
        let (name, value) = match part.split_once('=') {
            Some(pair) => pair,
            None => continue,
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "endpoint" => endpoint = Some(value.trim().trim_end_matches('/').to_string()),
            "accesskey" => {
                // `split_once` stops at the first `=`, and base64 padding is
                // `=` — so the value is the rest of the part, not just up to
                // the next one.
                access_key = Some(
                    BASE64
                        .decode(value.trim())
                        .map_err(|_| "the 'accesskey' in connection_string is not valid base64")?,
                )
            }
            _ => {}
        }
    }

    match (endpoint, access_key) {
        (Some(e), Some(k)) if !e.is_empty() && !k.is_empty() => Ok((e, k)),
        _ => Err(
            "connection_string must be 'endpoint=https://…;accesskey=…', as Azure gives it"
                .to_string(),
        ),
    }
}

fn host_of(endpoint: &str) -> Result<String, String> {
    let host = endpoint
        .strip_prefix("https://")
        .or_else(|| endpoint.strip_prefix("http://"))
        .ok_or("connection_string 'endpoint' must be an https:// URL")?
        .split('/')
        .next()
        .unwrap_or("");
    if host.is_empty() {
        return Err("connection_string 'endpoint' names no host".to_string());
    }
    Ok(host.to_string())
}

// ---------------------------------------------------------------------------
// Send
// ---------------------------------------------------------------------------

fn send(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let (endpoint, host, access_key, default_from) = {
        let guard = CONFIG.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = guard.as_ref().ok_or(NOT_CONFIGURED)?;
        (
            cfg.endpoint.clone(),
            cfg.host.clone(),
            cfg.access_key.clone(),
            cfg.from.clone(),
        )
    };

    let from = find_field(particle, "from")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or(default_from);

    let to = address_list(particle, "recipient")?;
    if to.is_empty() {
        return Err("Send requires at least one 'recipient'".to_string());
    }

    let subject = find_field(particle, "subject")
        .and_then(read_str)
        .unwrap_or("");
    let text = find_field(particle, "text").and_then(read_str);
    let html = find_field(particle, "html").and_then(read_str);

    // `mailer`'s rule, kept: html wins, text otherwise, and neither sends an
    // empty body rather than refusing.
    let mut content = json!({ "subject": subject });
    match (text, html) {
        (_, Some(html)) => content["html"] = json!(html),
        (Some(text), None) => content["plainText"] = json!(text),
        (None, None) => content["plainText"] = json!(""),
    }

    let mut recipients = json!({ "to": addresses(&to) });
    let cc = address_list(particle, "cc")?;
    if !cc.is_empty() {
        recipients["cC"] = addresses(&cc);
    }
    let bcc = address_list(particle, "bcc")?;
    if !bcc.is_empty() {
        recipients["bCC"] = addresses(&bcc);
    }

    let body = json!({
        "senderAddress": from,
        "content": content,
        "recipients": recipients,
    })
    .to_string();

    let path = format!("/emails:send?api-version={API_VERSION}");
    let date = httpdate::fmt_http_date(SystemTime::now());
    let content_hash = BASE64.encode(Sha256::digest(body.as_bytes()));
    let signature = sign("POST", &path, &date, &host, &content_hash, &access_key)?;

    let mut response = ureq::post(&format!("{endpoint}{path}"))
        .config()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build()
        .header("Content-Type", "application/json")
        .header("x-ms-date", &date)
        .header("x-ms-content-sha256", &content_hash)
        .header(
            "Authorization",
            &format!(
                "HMAC-SHA256 SignedHeaders=x-ms-date;host;x-ms-content-sha256&Signature={signature}"
            ),
        )
        .send(&body)
        .map_err(|e| format!("the request to '{host}' failed: {e}"))?;

    let status = response.status().as_u16();
    let reply = response
        .body_mut()
        .read_to_string()
        .unwrap_or_else(|_| String::new());

    if !(200..300).contains(&status) {
        // Azure explains itself in the body; pass that through rather than a
        // status nobody can act on.
        return Err(format!("Azure refused the message: HTTP {status}: {}", trim(&reply)));
    }

    // Accepted for delivery, which is not delivery — a later bounce is
    // between Azure and the recipient, and this module never sees it. The
    // operation id it answers with is how that is followed up, so it is
    // handed back rather than dropped.
    let operation = serde_json::from_str::<Json>(&reply)
        .ok()
        .and_then(|v| v.get("id").and_then(Json::as_str).map(str::to_string))
        .unwrap_or_default();

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"SendResult");
    boolean(b.slot_mut(1), true);
    owned_str(b.slot_mut(2), &operation);
    object(out, &[c"_class", c"ok", c"operation"], &mut b);
    b.release_all();
    Ok(())
}

/// Azure's HMAC scheme: the key signs the verb, the path, and the date, host
/// and body hash it was sent with. Signing the body's hash is what stops a
/// captured signature being reused for a different message.
fn sign(
    method: &str,
    path: &str,
    date: &str,
    host: &str,
    content_hash: &str,
    access_key: &[u8],
) -> Result<String, String> {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(access_key)
        .map_err(|_| "the access key in connection_string cannot sign".to_string())?;
    mac.update(format!("{method}\n{path}\n{date};{host};{content_hash}").as_bytes());
    Ok(BASE64.encode(mac.finalize().into_bytes()))
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

fn require_str<'a>(particle: &'a CodeValue, name: &str, class: &str) -> Result<&'a str, String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{class} requires a non-empty string '{name}'"))
}

/// A `recipient`/`cc`/`bcc` field: a single string, an array of strings, or
/// absent. `mailer`'s rule, so the two take the same particle.
fn address_list(particle: &CodeValue, field: &str) -> Result<Vec<String>, String> {
    match find_field(particle, field) {
        None => Ok(Vec::new()),
        Some(v) if v.tag == CodeTag::Str => Ok(read_str(v)
            .filter(|s| !s.is_empty())
            .map(|s| vec![s.to_string()])
            .unwrap_or_default()),
        Some(v) if v.tag == CodeTag::Array => array_elems(v)
            .map(|e| {
                read_str(e)
                    .map(str::to_string)
                    .ok_or_else(|| format!("every '{field}' address must be a string"))
            })
            .collect(),
        Some(_) => Err(format!(
            "'{field}' must be a string or an array of strings"
        )),
    }
}

fn addresses(list: &[String]) -> Json {
    Json::Array(list.iter().map(|a| json!({ "address": a })).collect())
}

/// A provider's error body, short enough to read in a log.
fn trim(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= 400 {
        return s.to_string();
    }
    let cut: String = s.chars().take(400).collect();
    format!("{cut}…")
}

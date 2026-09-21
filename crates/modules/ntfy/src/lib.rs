//! The `ntfy` native module — a push notification to a phone, for the Code
//! programming language, written in Rust on [`code-native`] over `ureq`.
//!
//! [ntfy](https://ntfy.sh) is a topic on an HTTP server: a `POST` to
//! `https://ntfy.sh/<topic>` with the message as the body reaches every
//! phone subscribed to that topic, at once, with no account. Title,
//! priority and tags ride as headers. A server of your own speaks the same
//! protocol. Anyone who knows a topic's name can read it — a topic is a
//! password; keep it in the environment, not the source.
//!
//! Handlers:
//!
//! - `Config { topic, url?, token?, username?, password?, timeout_seconds? }`
//!   → `ConfigResult { ok }` — the setup particle: which server and topic.
//!   `url` is `https://ntfy.sh` unless said. `token` (an ntfy access token)
//!   or `username` + `password` when the server asks for them.
//! - `Notify { message, title?, priority?, tags?, click?, topic?, markdown? }`
//!   → `Notified { ok, id }` — one notification. `priority` is 1–5 or
//!   `min` / `low` / `default` / `high` / `urgent` (`max`); `tags` a string
//!   or an array of strings (emoji short codes become icons on the phone);
//!   `click` a URL the notification opens; `topic` overrides Config's for
//!   this one. The server refusing, or not answering, is an `Exception`
//!   carrying its words — nothing is queued for later.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use code_native::*;
use std::sync::Mutex;
use std::time::Duration;

/// What `Config` set: where notifications go.
struct Setup {
    url: String,
    topic: String,
    auth: Option<String>,
    timeout: Duration,
}

static SETUP: Mutex<Option<Setup>> = Mutex::new(None);

const DEFAULT_URL: &str = "https://ntfy.sh";
const DEFAULT_TIMEOUT_SECONDS: f64 = 10.0;

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
    guarded(&mut *out, "ntfy", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Config" => config(out, particle),
            "Notify" => notify(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "ntfy", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// `Config { topic, url?, token?, username?, password?, timeout_seconds? }`
/// → `ConfigResult { ok }`.
fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let topic = require_str(particle, "topic", "Config")?;
    check_topic(topic)?;

    let url = find_field(particle, "url")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_URL)
        .trim_end_matches('/')
        .to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!("'url' must start with http:// or https://, not '{url}'"));
    }

    let auth = match (
        find_field(particle, "token").and_then(read_str).filter(|s| !s.is_empty()),
        find_field(particle, "username").and_then(read_str),
        find_field(particle, "password").and_then(read_str),
    ) {
        (Some(token), _, _) => Some(format!("Bearer {token}")),
        (None, Some(u), Some(p)) => Some(format!("Basic {}", base64(format!("{u}:{p}").as_bytes()))),
        (None, Some(_), None) | (None, None, Some(_)) => {
            return Err("ntfy auth needs both 'username' and 'password', or a 'token'".to_string())
        }
        (None, None, None) => None,
    };

    let timeout = match find_field(particle, "timeout_seconds") {
        None => DEFAULT_TIMEOUT_SECONDS,
        Some(v) => {
            let n = read_number(v).ok_or("'timeout_seconds' must be a number")?;
            if !(n > 0.0) {
                return Err("'timeout_seconds' must be above zero".to_string());
            }
            n
        }
    };

    *SETUP.lock().unwrap_or_else(|e| e.into_inner()) = Some(Setup {
        url,
        topic: topic.to_string(),
        auth,
        timeout: Duration::from_secs_f64(timeout),
    });

    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"ConfigResult");
    boolean(b.slot_mut(1), true);
    object(out, &[c"_class", c"ok"], &mut b);
    b.release_all();
    Ok(())
}

/// A topic is one path segment: letters, digits, `-` and `_`, as ntfy has it.
fn check_topic(topic: &str) -> Result<(), String> {
    if topic.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        Ok(())
    } else {
        Err(format!("'topic' may hold letters, digits, '-' and '_' only, not '{topic}'"))
    }
}

/// Standard base64 for the Basic header — small enough not to be worth a crate.
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |acc, &b| (acc << 8) | b as u32) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Notify
// ---------------------------------------------------------------------------

/// `Notify { message, title?, priority?, tags?, click?, topic?, markdown? }`
/// → `Notified { ok, id }`.
fn notify(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let guard = SETUP.lock().unwrap_or_else(|e| e.into_inner());
    let setup = guard
        .as_ref()
        .ok_or("ntfy has no server — send Config { topic } first")?;

    let message = require_str(particle, "message", "Notify")?;
    let topic = match find_field(particle, "topic").and_then(read_str).filter(|s| !s.is_empty()) {
        Some(t) => {
            check_topic(t)?;
            t
        }
        None => setup.topic.as_str(),
    };

    let mut headers: Vec<(&str, String)> = Vec::new();
    if let Some(title) = find_field(particle, "title").and_then(read_str).filter(|s| !s.is_empty()) {
        headers.push(("Title", header_safe(title)));
    }
    if let Some(v) = find_field(particle, "priority") {
        headers.push(("Priority", priority(v)?));
    }
    let tags = tag_list(particle)?;
    if !tags.is_empty() {
        headers.push(("Tags", tags.join(",")));
    }
    if let Some(click) = find_field(particle, "click").and_then(read_str).filter(|s| !s.is_empty()) {
        headers.push(("Click", header_safe(click)));
    }
    if read_field_bool(particle, "markdown").unwrap_or(false) {
        headers.push(("Markdown", "yes".to_string()));
    }
    if let Some(auth) = &setup.auth {
        headers.push(("Authorization", auth.clone()));
    }

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(setup.timeout))
        // 4xx/5xx come back as a response: the server's words are the
        // exception's words.
        .http_status_as_error(false)
        .build()
        .into();
    let mut request = agent
        .post(&format!("{}/{}", setup.url, topic))
        .header("Content-Type", "text/plain; charset=utf-8");
    for (name, value) in &headers {
        request = request.header(*name, value.as_str());
    }
    let mut response = request
        .send(message.as_bytes())
        .map_err(|e| format!("ntfy at {} did not answer: {e}", setup.url))?;

    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .with_config()
        .limit(64 * 1024)
        .read_to_string()
        .unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(format!("ntfy refused the notification ({status}): {}", body.trim()));
    }

    // ntfy answers the message as JSON; its `id` is worth handing back.
    let id = json_string_field(&body, "id").unwrap_or_default();
    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"Notified");
    boolean(b.slot_mut(1), true);
    owned_str(b.slot_mut(2), &id);
    object(out, &[c"_class", c"ok", c"id"], &mut b);
    b.release_all();
    Ok(())
}

/// `priority`: a number 1–5, or one of ntfy's names.
fn priority(v: &CodeValue) -> Result<String, String> {
    if let Some(n) = read_number(v) {
        if n.fract() != 0.0 || !(1.0..=5.0).contains(&n) {
            return Err("'priority' must be a whole number from 1 (min) to 5 (max)".to_string());
        }
        return Ok((n as u8).to_string());
    }
    match read_str(v) {
        Some(s @ ("min" | "low" | "default" | "high" | "urgent" | "max")) => Ok(s.to_string()),
        Some(other) => Err(format!(
            "'priority' must be 1–5 or min/low/default/high/urgent/max, not '{other}'"
        )),
        None => Err("'priority' must be a number or a string".to_string()),
    }
}

/// `tags`: a string, or an array of strings; each a header-safe word.
fn tag_list(particle: &CodeValue) -> Result<Vec<String>, String> {
    let one = |v: &CodeValue| -> Result<String, String> {
        let s = read_str(v).ok_or("every tag must be a string")?;
        if s.contains(',') || s.chars().any(|c| c.is_control()) {
            return Err(format!("a tag is one word, not '{s}'"));
        }
        Ok(s.to_string())
    };
    match find_field(particle, "tags") {
        None => Ok(Vec::new()),
        Some(v) if v.tag == CodeTag::Str => Ok(vec![one(v)?]),
        Some(v) if v.tag == CodeTag::Array => array_elems(v).map(one).collect(),
        Some(_) => Err("'tags' must be a string or an array of strings".to_string()),
    }
}

/// A header value cannot carry a line break; the rest ntfy takes as-is
/// (UTF-8 in a title is fine — ntfy reads it — but a newline ends the header).
fn header_safe(s: &str) -> String {
    s.chars().map(|c| if c == '\r' || c == '\n' { ' ' } else { c }).collect()
}

/// The string under `"key":` in a flat JSON object — enough for ntfy's
/// answer, and not worth a parser.
fn json_string_field(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let rest = &body[body.find(&needle)? + needle.len()..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn require_str<'a>(particle: &'a CodeValue, name: &str, class: &str) -> Result<&'a str, String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{class} requires a non-empty string '{name}'"))
}

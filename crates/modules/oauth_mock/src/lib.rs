//! The `oauth_mock` native module — a drop-in for `oauth` that talks to no
//! provider, for the Code programming language, written in Rust on
//! [`code-native`].
//!
//! Same particles and result shapes as `oauth`:
//!
//! - `Config { client_id, client_secret, redirect_uri, auth_url, token_url,
//!   userinfo_url?, scope? }` → `ConfigResult { ok }` — every field accepted,
//!   only `auth_url`/`redirect_uri`/`scope` used (to shape `AuthUrl`).
//! - `AuthUrl { state, extra? }` / `BuildAuthUrl` → `AuthUrlResult { url }` —
//!   `{auth_url}?state=…&redirect_uri=…&scope=…`, pointing at whatever
//!   `auth_url` was configured (a local mock-provider page, typically).
//! - `ExchangeCode { code }` → `Identity { sub, email, name, picture,
//!   access_token, refresh_token }` — the identity comes **from the code**:
//!   a URL-safe base64 of JSON `{ sub, email, name?, picture? }` if the code
//!   is one, otherwise a deterministic identity derived from the code
//!   string. No HTTP either way.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use base64::engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use code_native::*;
use serde_json::Value as Json;
use std::sync::Mutex;

#[derive(Default)]
struct Config {
    redirect_uri: String,
    auth_url: String,
    scope: String,
}

static CONFIG: Mutex<Option<Config>> = Mutex::new(None);

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
    guarded(&mut *out, "oauth_mock", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Config" => config(out, particle),
            "AuthUrl" | "BuildAuthUrl" => auth_url(out, particle),
            "ExchangeCode" => exchange_code(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "oauth_mock", &message);
        }
    })
}

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    // `oauth` requires these five; a mock that took fewer would let a broken
    // manifest pass in dev.
    for name in [
        "client_id",
        "client_secret",
        "redirect_uri",
        "auth_url",
        "token_url",
    ] {
        require_str(particle, name, "Config")?;
    }
    *CONFIG.lock().unwrap_or_else(|e| e.into_inner()) = Some(Config {
        redirect_uri: opt(particle, "redirect_uri").unwrap_or_default(),
        auth_url: opt(particle, "auth_url").unwrap_or_default(),
        scope: opt(particle, "scope").unwrap_or_default(),
    });

    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"ConfigResult");
    boolean(b.slot_mut(1), true);
    object(out, &[c"_class", c"ok"], &mut b);
    b.release_all();
    Ok(())
}

fn auth_url(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let state = require_str(particle, "state", "AuthUrl")?;
    let guard = CONFIG.lock().unwrap_or_else(|e| e.into_inner());
    let cfg = guard.as_ref().ok_or(NOT_CONFIGURED)?;

    let mut query = format!(
        "response_type=code&state={}&redirect_uri={}",
        encode(state),
        encode(&cfg.redirect_uri),
    );
    if !cfg.scope.is_empty() {
        query.push_str(&format!("&scope={}", encode(&cfg.scope)));
    }
    if let Some(extra) = find_field(particle, "extra") {
        if extra.tag != CodeTag::Object {
            return Err("'extra' must be an object of string values".to_string());
        }
        for (key, value) in object_entries(extra) {
            if key == "_class" {
                continue;
            }
            let v = read_str(value).ok_or("every 'extra' value must be a string")?;
            query.push_str(&format!("&{}={}", encode(key), encode(v)));
        }
    }

    let url = format!("{}?{query}", cfg.auth_url);
    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"AuthUrlResult");
    owned_str(b.slot_mut(1), &url);
    object(out, &[c"_class", c"url"], &mut b);
    b.release_all();
    Ok(())
}

fn exchange_code(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let code = require_str(particle, "code", "ExchangeCode")?;
    let _ = CONFIG
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .ok_or(NOT_CONFIGURED)?;

    let (sub, email, name, picture) = identity_from_code(code);

    let mut b = SlotBuffer::new(7);
    borrowed_str(b.slot_mut(0), c"Identity");
    owned_str(b.slot_mut(1), &sub);
    owned_str(b.slot_mut(2), &email);
    owned_str(b.slot_mut(3), &name);
    owned_str(b.slot_mut(4), &picture);
    owned_str(b.slot_mut(5), &format!("mock-access-{sub}"));
    owned_str(b.slot_mut(6), &format!("mock-refresh-{sub}"));
    object(
        out,
        &[
            c"_class",
            c"sub",
            c"email",
            c"name",
            c"picture",
            c"access_token",
            c"refresh_token",
        ],
        &mut b,
    );
    b.release_all();
    Ok(())
}

/// `(sub, email, name, picture)` recovered from the code: a base64-JSON
/// blob if the code is one, otherwise synthesised from the code string so
/// any code works in a test.
fn identity_from_code(code: &str) -> (String, String, String, String) {
    for engine in [URL_SAFE_NO_PAD, URL_SAFE, STANDARD] {
        if let Ok(bytes) = engine.decode(code.trim()) {
            if let Ok(json) = serde_json::from_slice::<Json>(&bytes) {
                let s = |k: &str| json.get(k).and_then(Json::as_str).unwrap_or("").to_string();
                let (sub, email) = (s("sub"), s("email"));
                if !sub.is_empty() && !email.is_empty() {
                    return (sub, email, s("name"), s("picture"));
                }
            }
        }
    }
    let slug: String = code
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    (
        format!("mock|{slug}"),
        format!("{slug}@mock.test"),
        String::new(),
        String::new(),
    )
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

const NOT_CONFIGURED: &str = "oauth_mock has no provider — send Config { … } first";

fn require_str<'a>(particle: &'a CodeValue, name: &str, class: &str) -> Result<&'a str, String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{class} requires a non-empty string '{name}'"))
}

fn opt(particle: &CodeValue, name: &str) -> Option<String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Percent-encode per RFC 3986: only unreserved characters pass through.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

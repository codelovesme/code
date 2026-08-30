//! The `oauth` native module — the OAuth 2.0 authorization-code flow for a
//! single provider, for the Code programming language, written in Rust on
//! [`code-native`].
//!
//! Server-side authorization code: the program builds the redirect URL, the
//! user comes back with a `code`, the program exchanges it for tokens and
//! (optionally) the user's identity. Secrets live in `Config`, delivered
//! from a manifest — a gene never sees the client secret.
//!
//! Handlers:
//!
//! - `Config { client_id, client_secret, redirect_uri, auth_url, token_url,
//!   userinfo_url?, scope? }` → `ConfigResult { ok }` — the provider. The
//!   setup particle: the others are an `Exception` until it has run.
//! - `AuthUrl { state, extra? }` → `AuthUrlResult { url }` — the
//!   authorization URL to redirect the user to. `extra` is `{ key = "value" }`
//!   for provider-specific parameters (`access_type = "offline"` for Google).
//! - `ExchangeCode { code }` → `Identity { sub, email, name, picture,
//!   access_token, refresh_token }` — trade the code for tokens, then fetch
//!   userinfo if `userinfo_url` is set (otherwise `sub`/`email`/… are empty).
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use code_native::*;
use serde_json::Value as Json;
use std::sync::Mutex;
use std::time::Duration;

static CONFIG: Mutex<Option<Provider>> = Mutex::new(None);

struct Provider {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    auth_url: String,
    token_url: String,
    userinfo_url: Option<String>,
    scope: String,
}

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
    guarded(&mut *out, "oauth", |out| {
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
            exception(out, "oauth", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let provider = Provider {
        client_id: require_str(particle, "client_id", "Config")?.to_string(),
        client_secret: require_str(particle, "client_secret", "Config")?.to_string(),
        redirect_uri: require_str(particle, "redirect_uri", "Config")?.to_string(),
        auth_url: require_str(particle, "auth_url", "Config")?.to_string(),
        token_url: require_str(particle, "token_url", "Config")?.to_string(),
        userinfo_url: find_field(particle, "userinfo_url")
            .and_then(read_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        scope: find_field(particle, "scope")
            .and_then(read_str)
            .unwrap_or("")
            .to_string(),
    };
    *CONFIG.lock().unwrap_or_else(|e| e.into_inner()) = Some(provider);

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
        "response_type=code&client_id={}&redirect_uri={}&state={}",
        encode(&cfg.client_id),
        encode(&cfg.redirect_uri),
        encode(state),
    );
    if !cfg.scope.is_empty() {
        query.push_str(&format!("&scope={}", encode(&cfg.scope)));
    }
    if let Some(extra) = find_field(particle, "extra") {
        if extra.tag != CodeTag::Object {
            return Err("'extra' must be an object of string values".to_string());
        }
        for (key, value) in object_entries(extra) {
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
    let guard = CONFIG.lock().unwrap_or_else(|e| e.into_inner());
    let cfg = guard.as_ref().ok_or(NOT_CONFIGURED)?;

    let body = format!(
        "grant_type=authorization_code&code={}&client_id={}&client_secret={}&redirect_uri={}",
        encode(code),
        encode(&cfg.client_id),
        encode(&cfg.client_secret),
        encode(&cfg.redirect_uri),
    );
    let token: Json = post_form(&cfg.token_url, &body)?;
    let access_token = token
        .get("access_token")
        .and_then(Json::as_str)
        .ok_or_else(|| {
            let hint = token
                .get("error_description")
                .or_else(|| token.get("error"))
                .and_then(Json::as_str)
                .unwrap_or("no access_token in the response");
            format!("token exchange rejected: {hint}")
        })?
        .to_string();
    let refresh_token = token
        .get("refresh_token")
        .and_then(Json::as_str)
        .unwrap_or("")
        .to_string();

    let (sub, email, name, picture) = match &cfg.userinfo_url {
        Some(url) => {
            let info = get_bearer(url, &access_token)?;
            (
                claim(&info, "sub"),
                claim(&info, "email"),
                claim(&info, "name"),
                claim(&info, "picture"),
            )
        }
        None => (String::new(), String::new(), String::new(), String::new()),
    };

    let mut b = SlotBuffer::new(7);
    borrowed_str(b.slot_mut(0), c"Identity");
    owned_str(b.slot_mut(1), &sub);
    owned_str(b.slot_mut(2), &email);
    owned_str(b.slot_mut(3), &name);
    owned_str(b.slot_mut(4), &picture);
    owned_str(b.slot_mut(5), &access_token);
    owned_str(b.slot_mut(6), &refresh_token);
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

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build()
        .into()
}

fn post_form(url: &str, body: &str) -> Result<Json, String> {
    let mut resp = agent()
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .send(body)
        .map_err(|e| format!("token endpoint '{url}' unreachable: {e}"))?;
    finish(resp.status().as_u16(), resp.body_mut().read_to_string())
}

fn get_bearer(url: &str, token: &str) -> Result<Json, String> {
    let mut resp = agent()
        .get(url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", "application/json")
        .call()
        .map_err(|e| format!("userinfo endpoint '{url}' unreachable: {e}"))?;
    finish(resp.status().as_u16(), resp.body_mut().read_to_string())
}

fn finish(status: u16, body: Result<String, ureq::Error>) -> Result<Json, String> {
    let text = body.map_err(|e| format!("reading the response failed: {e}"))?;
    let json: Json =
        serde_json::from_str(&text).map_err(|_| format!("HTTP {status}: {}", trim(&text)))?;
    if status >= 400 {
        let hint = json
            .get("error_description")
            .or_else(|| json.get("error"))
            .and_then(Json::as_str)
            .unwrap_or_else(|| trim(&text));
        return Err(format!("HTTP {status}: {hint}"));
    }
    Ok(json)
}

fn trim(s: &str) -> &str {
    let s = s.trim();
    if s.len() > 200 {
        &s[..200]
    } else {
        s
    }
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

const NOT_CONFIGURED: &str = "oauth has no provider — send Config { … } first";

fn require_str<'a>(particle: &'a CodeValue, name: &str, class: &str) -> Result<&'a str, String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{class} requires a non-empty string '{name}'"))
}

fn claim(info: &Json, key: &str) -> String {
    info.get(key)
        .and_then(Json::as_str)
        .unwrap_or("")
        .to_string()
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

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn encode_leaves_unreserved_and_escapes_the_rest() {
        assert_eq!(encode("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(encode("openid email profile"), "openid%20email%20profile");
        assert_eq!(
            encode("https://app/cb?x=1"),
            "https%3A%2F%2Fapp%2Fcb%3Fx%3D1"
        );
    }
}

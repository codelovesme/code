//! The `jwt` native module — HS256 JSON Web Tokens, for the Code programming
//! language, written in Rust on [`code-native`].
//!
//! HS256 only: a token is `base64url(header) . base64url(claims) .
//! base64url(HMAC-SHA256(secret, header.claims))`. That is small enough to
//! do directly, so this module pulls `hmac`/`sha2`/`base64` rather than
//! `jsonwebtoken` and a crypto backend.
//!
//! Handlers:
//!
//! - `Config { secret, expires_in? }` → `ConfigResult { ok }` — the signing
//!   secret and the default token lifetime in seconds (default 86400). A
//!   missing or empty `secret` is an `Exception`: nothing this module does
//!   works without one, so it is a stateful module and this is its setup
//!   particle.
//! - `Sign { sub, role?, expires_in? }` → `SignResult { token }` — a signed
//!   token carrying `{ sub, role, iat, exp }`. `Config` must have run first.
//! - `Decode { token }` → `DecodeResult { valid, sub, role, exp }` — a
//!   token that fails signature or expiry checks comes back
//!   `valid = false` (an answer, not an error). `Config` must have run first;
//!   a missing `token` is an `Exception`.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use code_native::*;
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use std::ffi::CStr;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

/// `{"alg":"HS256","typ":"JWT"}`, the only header this module writes or
/// accepts — precomputed so `Sign` never re-encodes it.
const HEADER_B64: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";

struct Config {
    secret: Vec<u8>,
    expires_in: u64,
}

static STATE: Mutex<Option<Config>> = Mutex::new(None);

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
    guarded(&mut *out, "jwt", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Config" => config(out, particle),
            "Sign" => sign(out, particle),
            "Decode" => decode(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "jwt", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `Config { secret, expires_in? }` → `ConfigResult { ok }`. The setup
/// particle: `Sign`/`Decode` are an `Exception` until it has run.
fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let secret = find_field(particle, "secret")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or("Config requires a non-empty string 'secret'")?;
    let expires_in = match find_field(particle, "expires_in") {
        None => 86_400,
        Some(v) => {
            let n = read_number(v).ok_or("'expires_in' must be a number")?;
            if n.fract() != 0.0 || n <= 0.0 {
                return Err("'expires_in' must be a positive whole number of seconds".to_string());
            }
            n as u64
        }
    };
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Config {
        secret: secret.as_bytes().to_vec(),
        expires_in,
    });
    one_field(out, c"ConfigResult", c"ok", |slot| boolean(slot, true));
    Ok(())
}

/// `Sign { sub, role?, expires_in? }` → `SignResult { token }`.
fn sign(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let sub = find_field(particle, "sub")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or("Sign requires a non-empty string 'sub'")?;
    let role = find_field(particle, "role").and_then(read_str).unwrap_or("");

    let guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let config = guard
        .as_ref()
        .ok_or("jwt has no secret — send Config { secret } first")?;

    let ttl = match find_field(particle, "expires_in") {
        None => config.expires_in,
        Some(v) => {
            let n = read_number(v).ok_or("'expires_in' must be a number")?;
            if n.fract() != 0.0 || n <= 0.0 {
                return Err("'expires_in' must be a positive whole number of seconds".to_string());
            }
            n as u64
        }
    };

    let now = unix_now();
    let claims = json!({ "sub": sub, "role": role, "iat": now, "exp": now + ttl });
    let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
    let signing_input = format!("{HEADER_B64}.{payload_b64}");
    let sig_b64 = URL_SAFE_NO_PAD.encode(hmac(&config.secret, signing_input.as_bytes()));

    let token = format!("{signing_input}.{sig_b64}");
    one_field(out, c"SignResult", c"token", |slot| owned_str(slot, &token));
    Ok(())
}

/// `Decode { token }` → `DecodeResult { valid, sub, role, exp }`. Signature
/// and expiry failures are `valid = false`, not exceptions — that is the
/// question `Decode` exists to answer.
fn decode(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let token = find_field(particle, "token")
        .and_then(read_str)
        .ok_or("Decode requires a string 'token'")?;

    let guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let config = guard
        .as_ref()
        .ok_or("jwt has no secret — send Config { secret } first")?;

    match verify(&config.secret, token) {
        Some(claims) => decode_result(
            out,
            true,
            claims.get("sub").and_then(Value::as_str).unwrap_or(""),
            claims.get("role").and_then(Value::as_str).unwrap_or(""),
            claims.get("exp").and_then(Value::as_i64).unwrap_or(0),
        ),
        None => decode_result(out, false, "", "", 0),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// HS256
// ---------------------------------------------------------------------------

fn hmac(secret: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

/// Verify a token's signature and expiry against `secret`. `Some(claims)`
/// only if the header is this module's, the signature matches, and `exp`
/// (when present) is still in the future.
fn verify(secret: &[u8], token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let (header_b64, payload_b64, sig_b64) =
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(h), Some(p), Some(s), None) => (h, p, s),
            _ => return None,
        };
    if header_b64 != HEADER_B64 {
        return None;
    }
    let signing_input = format!("{header_b64}.{payload_b64}");

    // `verify_slice` is a constant-time compare against the freshly computed
    // MAC — never decode the presented signature and `==` it.
    let presented = URL_SAFE_NO_PAD.decode(sig_b64).ok()?;
    let mut mac = HmacSha256::new_from_slice(secret).ok()?;
    mac.update(signing_input.as_bytes());
    mac.verify_slice(&presented).ok()?;

    let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload_b64).ok()?).ok()?;
    if let Some(exp) = claims.get("exp").and_then(Value::as_i64) {
        if exp <= unix_now() as i64 {
            return None;
        }
    }
    Some(claims)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Result shapes
// ---------------------------------------------------------------------------

fn decode_result(out: &mut CodeValue, valid: bool, sub: &str, role: &str, exp: i64) {
    let mut buf = SlotBuffer::new(5);
    borrowed_str(buf.slot_mut(0), c"DecodeResult");
    boolean(buf.slot_mut(1), valid);
    owned_str(buf.slot_mut(2), sub);
    owned_str(buf.slot_mut(3), role);
    number(buf.slot_mut(4), exp as f64);
    object(out, &[c"_class", c"valid", c"sub", c"role", c"exp"], &mut buf);
    buf.release_all();
}

/// `{ _class = <class>, <key> = <fill's result> }` — a result with one named
/// field.
fn one_field(
    out: &mut CodeValue,
    class: &'static CStr,
    key: &'static CStr,
    fill: impl FnOnce(&mut CodeValue),
) {
    let mut buf = SlotBuffer::new(2);
    borrowed_str(buf.slot_mut(0), class);
    fill(buf.slot_mut(1));
    object(out, &[c"_class", key], &mut buf);
    buf.release_all();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a token by hand, the same way `sign` does, so a test can choose
    /// its own `exp`.
    fn token_with_exp(secret: &[u8], exp: i64) -> String {
        let claims = json!({ "sub": "u", "role": "", "iat": 0, "exp": exp });
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        let signing_input = format!("{HEADER_B64}.{payload}");
        let sig = URL_SAFE_NO_PAD.encode(hmac(secret, signing_input.as_bytes()));
        format!("{signing_input}.{sig}")
    }

    #[test]
    fn a_valid_unexpired_token_verifies() {
        let secret = b"k";
        let t = token_with_exp(secret, unix_now() as i64 + 60);
        assert!(verify(secret, &t).is_some());
    }

    #[test]
    fn an_expired_token_does_not_verify() {
        let secret = b"k";
        let t = token_with_exp(secret, unix_now() as i64 - 1);
        assert!(verify(secret, &t).is_none());
    }

    #[test]
    fn the_wrong_secret_does_not_verify() {
        let t = token_with_exp(b"right", unix_now() as i64 + 60);
        assert!(verify(b"wrong", &t).is_none());
    }

    #[test]
    fn a_tampered_payload_does_not_verify() {
        let secret = b"k";
        let mut t = token_with_exp(secret, unix_now() as i64 + 60);
        t.insert(HEADER_B64.len() + 2, 'A'); // corrupt the payload segment
        assert!(verify(secret, &t).is_none());
    }

    #[test]
    fn a_token_with_no_exp_claim_verifies() {
        let secret = b"k";
        let claims = json!({ "sub": "u" });
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        let signing_input = format!("{HEADER_B64}.{payload}");
        let sig = URL_SAFE_NO_PAD.encode(hmac(secret, signing_input.as_bytes()));
        assert!(verify(secret, &format!("{signing_input}.{sig}")).is_some());
    }

    #[test]
    fn a_foreign_header_is_rejected_before_the_mac_check() {
        // `{"alg":"none"}` base64url — a classic downgrade attempt.
        let forged = "eyJhbGciOiJub25lIn0.eyJzdWIiOiJ1In0.";
        assert!(verify(b"k", forged).is_none());
    }
}

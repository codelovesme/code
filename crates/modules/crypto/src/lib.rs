//! The `crypto` native module — password hashing and random codes, for the
//! Code programming language, written in Rust on [`code-native`].
//!
//! Stateless: there is nothing to configure and no `Config` — a `cost` is a
//! per-call parameter with a sane default.
//!
//! Handlers:
//!
//! - `Hash { password, cost? }` → `HashResult { hash }` — a bcrypt hash.
//!   `cost` defaults to 12 (`bcrypt::DEFAULT_COST`).
//! - `Verify { password, hash }` → `VerifyResult { valid }` — whether the
//!   password matches. A wrong password is `valid = false`, not an error;
//!   only a malformed `hash` string is an `Exception`.
//! - `RandomCode { length? }` → `RandomCodeResult { code }` — a random
//!   `[A-Za-z0-9]` string, default length 32.
//!
//! ## Cost
//!
//! bcrypt's work factor. The default is 12; a per-call `cost` must be a whole
//! number in bcrypt's own 4..=31 range, else an `Exception`. Higher is slower
//! to hash *and* to attack.
//!
//! ## Randomness
//!
//! `RandomCode` draws from `rand::thread_rng()` — a ChaCha-based CSPRNG
//! seeded from the OS — with rejection sampling over the 62-character
//! alphabet, so every character is uniform. Length is clamped to 1..=512.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use bcrypt::{hash, verify, DEFAULT_COST};
use code_native::*;
use rand::Rng;
use std::ffi::CStr;

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
    guarded(&mut *out, "crypto", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Hash" => hash_handler(out, particle),
            "Verify" => verify_handler(out, particle),
            "RandomCode" => random_code(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "crypto", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `Hash { password, cost? }` → `HashResult { hash }`.
fn hash_handler(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let password = require_str(particle, "password", "Hash")?;
    let cost = read_cost(particle)?.unwrap_or(DEFAULT_COST);
    let hashed = hash(password, cost).map_err(|e| format!("bcrypt hash failed: {e}"))?;
    one_field(out, c"HashResult", c"hash", |slot| owned_str(slot, &hashed));
    Ok(())
}

/// `Verify { password, hash }` → `VerifyResult { valid }`. A wrong password
/// is `valid = false`; a `hash` that isn't a bcrypt string is an
/// `Exception`, since the program asked a question the input can't answer.
fn verify_handler(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let password = require_str(particle, "password", "Verify")?;
    let hash_str = require_str(particle, "hash", "Verify")?;
    let valid = verify(password, hash_str).map_err(|e| format!("bcrypt verify failed: {e}"))?;
    one_field(out, c"VerifyResult", c"valid", |slot| boolean(slot, valid));
    Ok(())
}

/// `RandomCode { length? }` → `RandomCodeResult { code }`.
fn random_code(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let length = match find_field(particle, "length") {
        None => 32,
        Some(v) => read_number(v)
            .filter(|n| n.fract() == 0.0 && *n >= 0.0)
            .ok_or("RandomCode 'length' must be a whole number, 0 or greater")? as usize,
    }
    .clamp(1, 512);

    let mut rng = rand::thread_rng();
    let code: String = (0..length)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect();
    one_field(out, c"RandomCodeResult", c"code", |slot| owned_str(slot, &code));
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

/// A required string field. A field the particle does not carry reads as
/// null — the same answer `.field` gives — so "missing" and "wrong type"
/// are one check, not two. An empty string is still a string: bcrypt hashes
/// `""` fine and the program is entitled to ask.
fn require_str<'a>(particle: &'a CodeValue, name: &str, class: &str) -> Result<&'a str, String> {
    find_field(particle, name)
        .and_then(read_str)
        .ok_or_else(|| format!("{class} requires a string '{name}'"))
}

/// `cost`, if present, as a bcrypt work factor. Absent → `Ok(None)`;
/// present but not a whole number in 4..=31 → an error naming the range.
fn read_cost(particle: &CodeValue) -> Result<Option<u32>, String> {
    match find_field(particle, "cost") {
        None => Ok(None),
        Some(v) => {
            let n = read_number(v).ok_or("'cost' must be a number")?;
            if n.fract() != 0.0 || !(4.0..=31.0).contains(&n) {
                return Err("'cost' must be a whole number in 4..=31".to_string());
            }
            Ok(Some(n as u32))
        }
    }
}

/// Build `{ _class = <class>, <key> = <fill's result> }` — a result with one
/// named field, the shape these handlers answer with (`HashResult { hash }`
/// rather than the generic `{ value }` that `make_result` gives).
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

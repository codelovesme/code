//! The `uuid` native module — time-ordered unique IDs, for the Code
//! programming language, written in Rust on [`code-native`].
//!
//! Stateless: there is nothing to configure and no `Config`.
//!
//! Handlers:
//!
//! - `V7 {}` → `UuidResult { value }` — a new version-7 UUID, lowercase and
//!   hyphenated (`0192f0c4-8b1e-7c3a-9d2f-5a6b7c8d9e0f`).
//! - `Parse { value }` → `ParsedUuid { value, version, unix_ms }` — a UUID
//!   read back: its canonical spelling, its version, and for a version 7 the
//!   millisecond it was made (null for any other version). Anything that is
//!   not a UUID is an `Exception`.
//!
//! ## Why version 7
//!
//! The first 48 bits are the Unix time in milliseconds, the rest random. So
//! IDs sort by when they were made — a database index stays in order and a
//! directory listing reads oldest first — and two machines never need to
//! agree on a counter. Within one process the `uuid` crate keeps them
//! strictly increasing even when two are made in the same millisecond.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use code_native::*;
use std::ffi::CStr;
use uuid_rs::Uuid;

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
    guarded(&mut *out, "uuid", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "V7" => v7(out),
            "Parse" => parse(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "uuid", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `V7 {}` → `UuidResult { value }`.
fn v7(out: &mut CodeValue) -> Result<(), String> {
    let made = Uuid::now_v7().hyphenated().to_string();
    let mut buf = SlotBuffer::new(2);
    borrowed_str(buf.slot_mut(0), c"UuidResult");
    owned_str(buf.slot_mut(1), &made);
    object(out, &[c"_class", c"value"], &mut buf);
    buf.release_all();
    Ok(())
}

/// `Parse { value }` → `ParsedUuid { value, version, unix_ms }`. Accepts
/// the spellings the `uuid` crate does — hyphenated, simple, braced, URN —
/// in either case; answers the canonical lowercase hyphenated one.
fn parse(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let given = find_field(particle, "value")
        .and_then(read_str)
        .ok_or("Parse requires a string 'value'")?;
    let id = Uuid::parse_str(given).map_err(|e| format!("not a UUID: {e}"))?;
    let unix_ms = id
        .get_timestamp()
        .filter(|_| id.get_version_num() == 7)
        .map(|t| {
            let (secs, nanos) = t.to_unix();
            secs as f64 * 1000.0 + (nanos / 1_000_000) as f64
        });

    let canonical = id.hyphenated().to_string();
    let mut buf = SlotBuffer::new(4);
    borrowed_str(buf.slot_mut(0), c"ParsedUuid");
    owned_str(buf.slot_mut(1), &canonical);
    number(buf.slot_mut(2), id.get_version_num() as f64);
    match unix_ms {
        Some(ms) => number(buf.slot_mut(3), ms),
        None => null(buf.slot_mut(3)),
    }
    let keys: [&'static CStr; 4] = [c"_class", c"value", c"version", c"unix_ms"];
    object(out, &keys, &mut buf);
    buf.release_all();
    Ok(())
}

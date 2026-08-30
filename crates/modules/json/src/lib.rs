//! The `json` native module — JSON text in and out, for the Code programming
//! language, written in Rust on [`code-native`].
//!
//! The language already renders any value as compact JSON through string
//! interpolation (`"$value"`), so this module is the two things interpolation
//! cannot do: **parse** a JSON string back into a value, and **pretty-print**
//! one. Handlers:
//!
//! - `Parse { text }` → `ParseResult { value }` — the parsed value. Invalid
//!   JSON, or a missing/non-string `text`, comes back as an `Exception`.
//! - `Stringify { value, pretty? }` → `StringifyResult { value }` — JSON
//!   text. `pretty = true` indents with two spaces; the default is compact
//!   and byte-for-byte what `"$value"` produces.
//!
//! ## `_class` is dropped
//!
//! Every particle and handler result carries a `_class` field the language
//! injects. `Stringify` drops it — from the top-level object and from nested
//! ones — so `Stringify { value = received }` gives the data the program is
//! actually carrying, not the plumbing. No other `_`-prefixed key is
//! touched: `_id` from a database row survives.
//!
//! ## Numbers
//!
//! The value model is JSON's, so there is one number type (`f64`). A whole
//! one is written without a fractional part (`1`, not `1.0`) — the same rule
//! interpolation follows — and `Parse` maps both `1` and `1.0` to the same
//! value. There is no bignum and no NaN/Infinity: JSON has no way to spell
//! them, and neither does this language.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use code_native::*;
use serde_json::Value as Json;

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
    guarded(&mut *out, "json", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Parse" => parse(out, particle),
            "Stringify" => stringify(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "json", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `Parse { text }` → `ParseResult { value }`.
fn parse(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let text = find_field(particle, "text")
        .and_then(read_str)
        .ok_or("Parse requires a string 'text'")?;
    let json: Json =
        serde_json::from_str(text).map_err(|e| format!("invalid JSON: {e}"))?;
    make_result(out, c"ParseResult", |slot| from_json(slot, &json));
    Ok(())
}

/// `Stringify { value, pretty? }` → `StringifyResult { value }`.
fn stringify(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    // `Stringify { }` stringifies null — there is no value this cannot
    // render, so there is nothing to reject.
    let json = match find_field(particle, "value") {
        Some(v) => to_json(v),
        None => Json::Null,
    };
    let pretty = read_field_bool(particle, "pretty").unwrap_or(false);
    let text = if pretty {
        serde_json::to_string_pretty(&json)
    } else {
        serde_json::to_string(&json)
    }
    .map_err(|e| format!("cannot serialize: {e}"))?;
    make_result(out, c"StringifyResult", |slot| owned_str(slot, &text));
    Ok(())
}

// ---------------------------------------------------------------------------
// CodeValue <-> serde_json::Value
// ---------------------------------------------------------------------------

/// A code value to its JSON form. `_class` is dropped wherever it appears;
/// every other key is kept.
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
                if key != "_class" {
                    map.insert(key.to_owned(), to_json(value));
                }
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
/// as an integer, matching the language's own "shortest form that
/// round-trips" rule for interpolation; everything else is a float. A
/// non-finite value has no JSON spelling, so it becomes `null` rather than
/// failing the whole document.
fn number_to_json(n: f64) -> Json {
    if !n.is_finite() {
        Json::Null
    } else if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
        Json::Number((n as i64).into())
    } else {
        serde_json::Number::from_f64(n).map_or(Json::Null, Json::Number)
    }
}

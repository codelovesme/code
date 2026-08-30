//! The `json_store` native module — a file-backed key-value store, for the
//! Code programming language, written in Rust on [`code-native`].
//!
//! One JSON file per key, under a base directory `Config` sets: `<base>/<key>.json`.
//! The files are readable and hand-editable on purpose — this is for
//! lightweight runtime state (which apps a user stopped, a feature flag, a
//! cached small document), not a database.
//!
//! Handlers:
//!
//! - `Config { base_dir }` → `ConfigResult { ok, base_dir }` — the store's
//!   directory, created if missing.
//! - `Store { key, value }` → `StoreResult { key }` — write `value` (any
//!   value; `Store { key }` writes `null`). The write is atomic.
//! - `Fetch { key }` → `FetchResult { exists, key, value }` — the stored
//!   value, or `{ exists = false, value = null }`.
//! - `Delete { key }` / `Remove { key }` → `DeleteResult { key, existed }` —
//!   idempotent.
//!
//! A key must be a non-empty run of `[A-Za-z0-9._:@-]` and not be `.` or
//! `..` — it becomes a filename, and anything else is an `Exception` rather
//! than a silent character substitution that could collide two keys onto one
//! file.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use code_native::*;
use serde_json::Value as Json;
use std::path::PathBuf;
use std::sync::Mutex;
use std::{fs, io};

static BASE: Mutex<Option<PathBuf>> = Mutex::new(None);

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
    guarded(&mut *out, "json_store", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Config" => config(out, particle),
            "Store" => store(out, particle),
            "Fetch" => fetch(out, particle),
            "Delete" | "Remove" => delete(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "json_store", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let raw = find_field(particle, "base_dir")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or("Config requires a non-empty string 'base_dir'")?;
    let dir = PathBuf::from(raw);
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create '{raw}': {e}"))?;
    let dir = fs::canonicalize(&dir).map_err(|e| format!("cannot resolve '{raw}': {e}"))?;
    let shown = dir.display().to_string();
    *BASE.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir);

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"ConfigResult");
    boolean(b.slot_mut(1), true);
    owned_str(b.slot_mut(2), &shown);
    object(out, &[c"_class", c"ok", c"base_dir"], &mut b);
    b.release_all();
    Ok(())
}

fn store(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let (key, path) = target(particle)?;
    let value = find_field(particle, "value").map_or(Json::Null, to_json);
    let text = serde_json::to_string_pretty(&value).map_err(|e| format!("cannot serialize: {e}"))?;

    // Atomic: a concurrent Fetch sees the old file or the whole new one.
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text).map_err(|e| format!("cannot write '{key}': {e}"))?;
    if let Err(e) = fs::rename(&tmp, &path) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("cannot commit '{key}': {e}"));
    }

    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"StoreResult");
    owned_str(b.slot_mut(1), &key);
    object(out, &[c"_class", c"key"], &mut b);
    b.release_all();
    Ok(())
}

fn fetch(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let (key, path) = target(particle)?;
    let (exists, value) = match fs::read_to_string(&path) {
        Ok(text) => {
            let json: Json =
                serde_json::from_str(&text).map_err(|e| format!("'{key}' is corrupt: {e}"))?;
            (true, Some(json))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => (false, None),
        Err(e) => return Err(format!("cannot read '{key}': {e}")),
    };

    let mut b = SlotBuffer::new(4);
    borrowed_str(b.slot_mut(0), c"FetchResult");
    boolean(b.slot_mut(1), exists);
    owned_str(b.slot_mut(2), &key);
    match &value {
        Some(json) => from_json(b.slot_mut(3), json),
        None => null(b.slot_mut(3)),
    }
    object(out, &[c"_class", c"exists", c"key", c"value"], &mut b);
    b.release_all();
    Ok(())
}

fn delete(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let (key, path) = target(particle)?;
    let existed = match fs::remove_file(&path) {
        Ok(()) => true,
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(format!("cannot delete '{key}': {e}")),
    };
    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"DeleteResult");
    owned_str(b.slot_mut(1), &key);
    boolean(b.slot_mut(2), existed);
    object(out, &[c"_class", c"key", c"existed"], &mut b);
    b.release_all();
    Ok(())
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// The `key` field, validated, and the file it maps to. `Err` if `Config`
/// hasn't run, `key` is missing, or `key` is not filename-safe.
fn target(particle: &CodeValue) -> Result<(String, PathBuf), String> {
    let key = find_field(particle, "key")
        .and_then(read_str)
        .ok_or("this handler requires a string 'key'")?;
    if key.is_empty() || key == "." || key == ".." {
        return Err(format!("'{key}' is not a usable key"));
    }
    if let Some(bad) = key
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':' | '@')))
    {
        return Err(format!(
            "key '{key}' contains '{bad}' — keys must be [A-Za-z0-9._:@-]"
        ));
    }
    let guard = BASE.lock().unwrap_or_else(|e| e.into_inner());
    let base = guard
        .as_ref()
        .ok_or("json_store has no base — send Config { base_dir } first")?;
    Ok((key.to_string(), base.join(format!("{key}.json"))))
}

// ---------------------------------------------------------------------------
// CodeValue <-> serde_json (same rules as the `json` module: `_class` is
// dropped, whole numbers write without a fractional part)
// ---------------------------------------------------------------------------

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

fn number_to_json(n: f64) -> Json {
    if !n.is_finite() {
        Json::Null
    } else if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
        Json::Number((n as i64).into())
    } else {
        serde_json::Number::from_f64(n).map_or(Json::Null, Json::Number)
    }
}

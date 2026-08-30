//! The `blob_storage_mock` native module — a drop-in for `blob_storage`
//! backed by an in-memory map, for the Code programming language, written in
//! Rust on [`code-native`].
//!
//! Same particles and result shapes as `blob_storage` — `Config` (every S3
//! field accepted and ignored), `Put`/`Upload`, `Get`/`Download`, `Delete`,
//! `List` — but objects live in a `HashMap` for the life of the process. A
//! full put/get/list/delete round trip works; nothing leaves memory.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use code_native::*;
use std::collections::BTreeMap;
use std::sync::Mutex;

struct Object {
    bytes: Vec<u8>,
    content_type: String,
}

static STORE: Mutex<Option<BTreeMap<String, Object>>> = Mutex::new(None);

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
    guarded(&mut *out, "blob_storage_mock", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Config" => config(out, particle),
            "Put" | "Upload" => put(out, particle),
            "Get" | "Download" => get(out, particle),
            "Delete" => delete(out, particle),
            "List" => list(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "blob_storage_mock", &message);
        }
    })
}

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    // `blob_storage` requires all three; keep the mock strict.
    for name in ["bucket", "access_key", "secret_key"] {
        require_str(particle, name, "Config")?;
    }
    *STORE.lock().unwrap_or_else(|e| e.into_inner()) = Some(BTreeMap::new());

    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"ConfigResult");
    boolean(b.slot_mut(1), true);
    object(out, &[c"_class", c"ok"], &mut b);
    b.release_all();
    Ok(())
}

fn put(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Put")?.to_string();
    let data = find_field(particle, "data")
        .and_then(read_str)
        .ok_or("Put requires a string 'data'")?;
    let bytes = if read_field_bool(particle, "base64").unwrap_or(false) {
        B64.decode(data.trim())
            .map_err(|e| format!("'data' is not valid base64: {e}"))?
    } else {
        data.as_bytes().to_vec()
    };
    let content_type = find_field(particle, "content_type")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("application/octet-stream")
        .to_string();

    STORE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
        .ok_or(NOT_CONFIGURED)?
        .insert(
            key.clone(),
            Object {
                bytes,
                content_type,
            },
        );

    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"PutResult");
    owned_str(b.slot_mut(1), &key);
    object(out, &[c"_class", c"key"], &mut b);
    b.release_all();
    Ok(())
}

fn get(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Get")?;
    let as_base64 = read_field_bool(particle, "base64").unwrap_or(false);
    let guard_owned;
    {
        let mut guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
        let map = guard.as_mut().ok_or(NOT_CONFIGURED)?;
        guard_owned = map.get(key).map(|o| {
            let data = if as_base64 {
                B64.encode(&o.bytes)
            } else {
                String::from_utf8_lossy(&o.bytes).into_owned()
            };
            (data, o.content_type.clone())
        });
    }

    let mut b = SlotBuffer::new(5);
    borrowed_str(b.slot_mut(0), c"GetResult");
    match guard_owned {
        Some((data, content_type)) => {
            boolean(b.slot_mut(1), true);
            owned_str(b.slot_mut(2), key);
            owned_str(b.slot_mut(3), &data);
            owned_str(b.slot_mut(4), &content_type);
        }
        None => {
            boolean(b.slot_mut(1), false);
            owned_str(b.slot_mut(2), key);
            owned_str(b.slot_mut(3), "");
            owned_str(b.slot_mut(4), "");
        }
    }
    object(
        out,
        &[c"_class", c"found", c"key", c"data", c"content_type"],
        &mut b,
    );
    b.release_all();
    Ok(())
}

fn delete(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Delete")?;
    let existed = STORE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
        .ok_or(NOT_CONFIGURED)?
        .remove(key)
        .is_some();

    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"DeleteResult");
    boolean(b.slot_mut(1), existed);
    object(out, &[c"_class", c"existed"], &mut b);
    b.release_all();
    Ok(())
}

fn list(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let prefix = find_field(particle, "prefix")
        .and_then(read_str)
        .unwrap_or("")
        .to_string();
    let keys: Vec<String> = {
        let mut guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
        let map = guard.as_mut().ok_or(NOT_CONFIGURED)?;
        map.keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect()
    };

    let mut arr = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(keys.len());
    for (i, k) in keys.iter().enumerate() {
        owned_str(buf.slot_mut(i as i64), k);
    }
    array(&mut arr, &mut buf);
    buf.release_all();

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"ListResult");
    copy(b.slot_mut(1), &arr);
    number(b.slot_mut(2), keys.len() as f64);
    object(out, &[c"_class", c"keys", c"count"], &mut b);
    b.release_all();
    release(&mut arr);
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

const NOT_CONFIGURED: &str =
    "blob_storage_mock is not configured — send Config { bucket, … } first";

fn require_str<'a>(particle: &'a CodeValue, name: &str, class: &str) -> Result<&'a str, String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{class} requires a non-empty string '{name}'"))
}

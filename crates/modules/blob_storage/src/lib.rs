//! The `blob_storage` native module — S3-compatible object storage, for the
//! Code programming language, written in Rust on [`code-native`] over
//! `rust-s3`.
//!
//! S3, because it is the interface every object store speaks now — AWS S3,
//! MinIO, Cloudflare R2, Backblaze B2, DigitalOcean Spaces. The euglena
//! organelle this replaces spoke Azure Blob's SharedKey REST API directly;
//! this reaches the rest of the world (Azure via its S3 gateway aside).
//!
//! Handlers:
//!
//! - `Config { bucket, access_key, secret_key, endpoint?, region?,
//!   path_style? }` → `ConfigResult { ok }` — the setup particle. `endpoint`
//!   for anything that isn't AWS; `path_style` defaults on when `endpoint`
//!   is set (MinIO wants it).
//! - `Put { key, data, content_type?, base64? }` → `PutResult { key }` —
//!   `base64 = true` decodes `data` from base64 first (the only way to store
//!   bytes the language can't hold directly).
//! - `Get { key, base64? }` → `GetResult { found, key, data, content_type }`
//!   — `base64 = true` returns the bytes base64-encoded; otherwise `data` is
//!   the object decoded as UTF-8 (lossily).
//! - `Delete { key }` → `DeleteResult { existed }`
//! - `List { prefix? }` → `ListResult { keys, count }`
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use code_native::*;
use s3::creds::Credentials;
use s3::{Bucket, Region};
use std::sync::Mutex;

static BUCKET: Mutex<Option<Box<Bucket>>> = Mutex::new(None);

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
    guarded(&mut *out, "blob_storage", |out| {
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
            exception(out, "blob_storage", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let name = require_str(particle, "bucket", "Config")?;
    let access = require_str(particle, "access_key", "Config")?;
    let secret = require_str(particle, "secret_key", "Config")?;
    let endpoint = find_field(particle, "endpoint")
        .and_then(read_str)
        .filter(|s| !s.is_empty());
    let region_name = find_field(particle, "region")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("us-east-1")
        .to_string();
    // MinIO and most self-hosted stores need path-style; AWS wants
    // virtual-hosted. Default follows whether a custom endpoint was given.
    let path_style = read_field_bool(particle, "path_style").unwrap_or(endpoint.is_some());

    let region = match endpoint {
        Some(ep) => Region::Custom {
            region: region_name,
            endpoint: ep.trim_end_matches('/').to_string(),
        },
        None => region_name
            .parse()
            .map_err(|e| format!("bad region '{region_name}': {e}"))?,
    };
    let creds = Credentials::new(Some(access), Some(secret), None, None, None)
        .map_err(|e| format!("bad credentials: {e}"))?;

    let mut bucket = Bucket::new(name, region.clone(), creds.clone())
        .map_err(|e| format!("cannot open bucket: {e}"))?;
    if path_style {
        bucket.set_path_style();
    }

    // `create = true` makes the bucket if it isn't there — the euglena
    // organelle's Sap did this ("ensure container exists"). Off by default:
    // creating a bucket is not what "connect to storage" usually means.
    if read_field_bool(particle, "create").unwrap_or(false) {
        let cfg = s3::BucketConfiguration::default();
        let created = if path_style {
            Bucket::create_with_path_style(name, region, creds, cfg)
        } else {
            Bucket::create(name, region, creds, cfg)
        }
        .map_err(|e| format!("cannot create bucket '{name}': {e}"))?;
        // 409 = it already exists, which is the outcome we wanted.
        if created.response_code >= 300 && created.response_code != 409 {
            return Err(format!(
                "cannot create bucket '{name}': HTTP {}",
                created.response_code
            ));
        }
    }

    *BUCKET.lock().unwrap_or_else(|e| e.into_inner()) = Some(bucket);
    ok_result(out, c"ConfigResult");
    Ok(())
}

fn put(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Put")?;
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
        .unwrap_or("application/octet-stream");

    let resp = bucket()?
        .put_object_with_content_type(key, &bytes, content_type)
        .map_err(|e| format!("Put failed: {e}"))?;
    check_status(resp.status_code(), "Put")?;

    one_str(out, c"PutResult", c"key", key);
    Ok(())
}

fn get(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Get")?;
    let as_base64 = read_field_bool(particle, "base64").unwrap_or(false);

    let resp = match bucket()?.get_object(key) {
        Ok(r) if r.status_code() == 404 => return not_found(out, key),
        Ok(r) => {
            check_status(r.status_code(), "Get")?;
            r
        }
        Err(s3::error::S3Error::HttpFailWithBody(404, _)) => return not_found(out, key),
        Err(e) => return Err(format!("Get failed: {e}")),
    };

    let bytes = resp.bytes();
    let data = if as_base64 {
        B64.encode(bytes)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    let content_type = resp
        .headers()
        .get("content-type")
        .cloned()
        .unwrap_or_default();

    let mut b = SlotBuffer::new(5);
    borrowed_str(b.slot_mut(0), c"GetResult");
    boolean(b.slot_mut(1), true);
    owned_str(b.slot_mut(2), key);
    owned_str(b.slot_mut(3), &data);
    owned_str(b.slot_mut(4), &content_type);
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
    let b = bucket()?;
    let existed = matches!(b.get_object(key), Ok(r) if r.status_code() < 300);
    if existed {
        let resp = b
            .delete_object(key)
            .map_err(|e| format!("Delete failed: {e}"))?;
        check_status(resp.status_code(), "Delete")?;
    }
    one_bool(out, c"DeleteResult", c"existed", existed);
    Ok(())
}

fn list(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let prefix = find_field(particle, "prefix")
        .and_then(read_str)
        .unwrap_or("")
        .to_string();
    let pages = bucket()?
        .list(prefix, None)
        .map_err(|e| format!("List failed: {e}"))?;
    let mut keys: Vec<String> = pages
        .into_iter()
        .flat_map(|p| p.contents.into_iter().map(|o| o.key))
        .collect();
    keys.sort();

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

fn bucket() -> Result<Box<Bucket>, String> {
    BUCKET
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or_else(|| {
            "blob_storage is not configured — send Config { bucket, … } first".to_string()
        })
}

fn check_status(code: u16, op: &str) -> Result<(), String> {
    if code >= 300 {
        Err(format!("{op} failed: HTTP {code}"))
    } else {
        Ok(())
    }
}

fn not_found(out: &mut CodeValue, key: &str) -> Result<(), String> {
    let mut b = SlotBuffer::new(5);
    borrowed_str(b.slot_mut(0), c"GetResult");
    boolean(b.slot_mut(1), false);
    owned_str(b.slot_mut(2), key);
    owned_str(b.slot_mut(3), "");
    owned_str(b.slot_mut(4), "");
    object(
        out,
        &[c"_class", c"found", c"key", c"data", c"content_type"],
        &mut b,
    );
    b.release_all();
    Ok(())
}

fn require_str<'a>(particle: &'a CodeValue, name: &str, class: &str) -> Result<&'a str, String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{class} requires a non-empty string '{name}'"))
}

fn ok_result(out: &mut CodeValue, class: &'static std::ffi::CStr) {
    one_bool(out, class, c"ok", true);
}

fn one_str(
    out: &mut CodeValue,
    class: &'static std::ffi::CStr,
    key: &'static std::ffi::CStr,
    value: &str,
) {
    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), class);
    owned_str(b.slot_mut(1), value);
    object(out, &[c"_class", key], &mut b);
    b.release_all();
}

fn one_bool(
    out: &mut CodeValue,
    class: &'static std::ffi::CStr,
    key: &'static std::ffi::CStr,
    value: bool,
) {
    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), class);
    boolean(b.slot_mut(1), value);
    object(out, &[c"_class", key], &mut b);
    b.release_all();
}

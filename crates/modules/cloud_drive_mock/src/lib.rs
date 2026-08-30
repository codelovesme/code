//! The `cloud_drive_mock` native module — a drop-in for `cloud_drive` that
//! reaches no Google, for the Code programming language, written in Rust on
//! [`code-native`].
//!
//! Same particles and result shapes as `cloud_drive`. The OAuth pair
//! recovers an identity from the code itself; the file operations run
//! against an in-memory store that lives for the process. A full
//! upload/list/download/delete round trip works.
//!
//! - `Config { client_id, client_secret, redirect_uri?, scope?, auth_url?,
//!   token_url?, api_base? }` → `ConfigResult { ok }`
//! - `AuthUrl` / `BuildAuthUrl` `{ state, redirect_uri?, extra? }` →
//!   `AuthUrlResult { url }`
//! - `ExchangeCode { code }` → `Tokens { account_email, access_token,
//!   refresh_token, expires_in }` — `account_email` comes from a base64-JSON
//!   code (`{ email }`) or is synthesised from the code string.
//! - `RefreshToken { refresh_token }` → `Tokens { … }`
//! - `GetQuota { access_token }` → `Quota { account_email, total, used,
//!   available }` — `total` is a fixed 16 GiB; `used` is the stored bytes.
//! - `ListFiles` / `UploadFile` / `DownloadFile` / `DeleteFile` — the
//!   in-memory store.
//!
//! A `provider` other than `"google"` is an `Exception`, as in the real
//! module.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use base64::engine::general_purpose::{STANDARD as B64, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use code_native::*;
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Default)]
struct Config {
    redirect_uri: String,
    scope: String,
    auth_url: String,
}

struct File {
    name: String,
    content_type: String,
    bytes: Vec<u8>,
}

const QUOTA_TOTAL: f64 = 16.0 * 1024.0 * 1024.0 * 1024.0;

static CONFIG: Mutex<Option<Config>> = Mutex::new(None);
static FILES: Mutex<Option<BTreeMap<String, File>>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

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
    guarded(&mut *out, "cloud_drive_mock", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Config" => config(out, particle),
            "AuthUrl" | "BuildAuthUrl" => auth_url(out, particle),
            "ExchangeCode" => exchange_code(out, particle),
            "RefreshToken" => refresh_token(out, particle),
            "GetQuota" => get_quota(out, particle),
            "ListFiles" => list_files(out, particle),
            "UploadFile" => upload_file(out, particle),
            "DownloadFile" => download_file(out, particle),
            "DeleteFile" => delete_file(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "cloud_drive_mock", &message);
        }
    })
}

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    require_str(particle, "client_id", "Config")?;
    require_str(particle, "client_secret", "Config")?;
    *CONFIG.lock().unwrap_or_else(|e| e.into_inner()) = Some(Config {
        redirect_uri: opt(particle, "redirect_uri").unwrap_or_default(),
        scope: opt(particle, "scope").unwrap_or_default(),
        auth_url: opt(particle, "auth_url")
            .unwrap_or_else(|| "https://accounts.google.com/o/oauth2/v2/auth".to_string()),
    });
    *FILES.lock().unwrap_or_else(|e| e.into_inner()) = Some(BTreeMap::new());

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
    let redirect = opt(particle, "redirect_uri").unwrap_or_else(|| cfg.redirect_uri.clone());

    let mut query = format!(
        "response_type=code&redirect_uri={}&state={}",
        encode(&redirect),
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
    ensure_configured()?;
    let email = email_from_code(code);
    tokens(out, &email, "mock-access", "mock-refresh", 3600.0);
    Ok(())
}

fn refresh_token(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let token = require_str(particle, "refresh_token", "RefreshToken")?;
    ensure_configured()?;
    tokens(out, "", "mock-access-refreshed", token, 3600.0);
    Ok(())
}

fn get_quota(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let _ = access_token(particle, "GetQuota")?;
    let used: f64 = with_files(|f| f.values().map(|x| x.bytes.len() as f64).sum())?;

    let mut b = SlotBuffer::new(5);
    borrowed_str(b.slot_mut(0), c"Quota");
    borrowed_str(b.slot_mut(1), c"mock@drive.test");
    number(b.slot_mut(2), QUOTA_TOTAL);
    number(b.slot_mut(3), used);
    number(b.slot_mut(4), (QUOTA_TOTAL - used).max(0.0));
    object(
        out,
        &[c"_class", c"account_email", c"total", c"used", c"available"],
        &mut b,
    );
    b.release_all();
    Ok(())
}

fn list_files(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let _ = access_token(particle, "ListFiles")?;
    let entries: Vec<(String, String, String, u64)> = with_files(|f| {
        f.iter()
            .map(|(id, x)| {
                (
                    id.clone(),
                    x.name.clone(),
                    x.content_type.clone(),
                    x.bytes.len() as u64,
                )
            })
            .collect()
    })?;

    let mut arr = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(entries.len());
    for (i, (id, name, ct, size)) in entries.iter().enumerate() {
        remote_file(buf.slot_mut(i as i64), id, name, ct, *size);
    }
    array(&mut arr, &mut buf);
    buf.release_all();

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"FileList");
    copy(b.slot_mut(1), &arr);
    number(b.slot_mut(2), entries.len() as f64);
    object(out, &[c"_class", c"files", c"count"], &mut b);
    b.release_all();
    release(&mut arr);
    Ok(())
}

fn upload_file(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let _ = access_token(particle, "UploadFile")?;
    let name = require_str(particle, "file_name", "UploadFile")?.to_string();
    let bytes = payload(particle)?;
    let content_type = opt(particle, "content_type")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let size = bytes.len() as u64;
    let id = format!("mock-file-{}", NEXT_ID.fetch_add(1, Ordering::SeqCst));

    with_files(|f| {
        f.insert(
            id.clone(),
            File {
                name: name.clone(),
                content_type: content_type.clone(),
                bytes: bytes.clone(),
            },
        )
    })?;

    remote_file(out, &id, &name, &content_type, size);
    Ok(())
}

fn download_file(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let _ = access_token(particle, "DownloadFile")?;
    let file_id = require_str(particle, "file_id", "DownloadFile")?;
    let as_base64 = read_field_bool(particle, "base64").unwrap_or(false);

    let found = with_files(|f| {
        f.get(file_id).map(|x| {
            let data = if as_base64 {
                B64.encode(&x.bytes)
            } else {
                String::from_utf8_lossy(&x.bytes).into_owned()
            };
            (x.name.clone(), x.content_type.clone(), data)
        })
    })?;
    let (name, content_type, data) = found.ok_or_else(|| format!("no file with id '{file_id}'"))?;

    let mut b = SlotBuffer::new(5);
    borrowed_str(b.slot_mut(0), c"FileContent");
    owned_str(b.slot_mut(1), file_id);
    owned_str(b.slot_mut(2), &name);
    owned_str(b.slot_mut(3), &content_type);
    owned_str(b.slot_mut(4), &data);
    object(
        out,
        &[
            c"_class",
            c"file_id",
            c"file_name",
            c"content_type",
            c"data",
        ],
        &mut b,
    );
    b.release_all();
    Ok(())
}

fn delete_file(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let _ = access_token(particle, "DeleteFile")?;
    let file_id = require_str(particle, "file_id", "DeleteFile")?;
    let existed = with_files(|f| f.remove(file_id).is_some())?;

    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"DeleteResult");
    boolean(b.slot_mut(1), existed);
    object(out, &[c"_class", c"existed"], &mut b);
    b.release_all();
    Ok(())
}

// ---------------------------------------------------------------------------
// Value building
// ---------------------------------------------------------------------------

fn tokens(out: &mut CodeValue, email: &str, access: &str, refresh: &str, expires: f64) {
    let mut b = SlotBuffer::new(5);
    borrowed_str(b.slot_mut(0), c"Tokens");
    owned_str(b.slot_mut(1), email);
    owned_str(b.slot_mut(2), access);
    owned_str(b.slot_mut(3), refresh);
    number(b.slot_mut(4), expires);
    object(
        out,
        &[
            c"_class",
            c"account_email",
            c"access_token",
            c"refresh_token",
            c"expires_in",
        ],
        &mut b,
    );
    b.release_all();
}

fn remote_file(out: &mut CodeValue, id: &str, name: &str, content_type: &str, size: u64) {
    let mut b = SlotBuffer::new(6);
    borrowed_str(b.slot_mut(0), c"RemoteFile");
    owned_str(b.slot_mut(1), id);
    owned_str(b.slot_mut(2), name);
    owned_str(b.slot_mut(3), content_type);
    number(b.slot_mut(4), size as f64);
    owned_str(b.slot_mut(5), &format!("mock://drive/{id}"));
    object(
        out,
        &[
            c"_class",
            c"file_id",
            c"file_name",
            c"content_type",
            c"size",
            c"web_view_url",
        ],
        &mut b,
    );
    b.release_all();
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

const NOT_CONFIGURED: &str = "cloud_drive_mock has no credentials — send Config { … } first";

fn ensure_configured() -> Result<(), String> {
    if CONFIG.lock().unwrap_or_else(|e| e.into_inner()).is_none() {
        return Err(NOT_CONFIGURED.to_string());
    }
    Ok(())
}

fn with_files<T>(f: impl FnOnce(&mut BTreeMap<String, File>) -> T) -> Result<T, String> {
    let mut guard = FILES.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.as_mut().ok_or(NOT_CONFIGURED)?;
    Ok(f(map))
}

fn access_token(particle: &CodeValue, class: &str) -> Result<String, String> {
    if let Some(p) = opt(particle, "provider") {
        if !p.is_empty() && p != "google" {
            return Err(format!(
                "cloud_drive_mock supports only Google Drive — got provider '{p}'"
            ));
        }
    }
    Ok(require_str(particle, "access_token", class)?.to_string())
}

fn email_from_code(code: &str) -> String {
    for engine in [URL_SAFE_NO_PAD, URL_SAFE, B64] {
        if let Ok(bytes) = engine.decode(code.trim()) {
            if let Ok(json) = serde_json::from_slice::<Json>(&bytes) {
                if let Some(email) = json.get("email").and_then(Json::as_str) {
                    if !email.is_empty() {
                        return email.to_string();
                    }
                }
            }
        }
    }
    let slug: String = code
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("{slug}@drive.test")
}

fn payload(particle: &CodeValue) -> Result<Vec<u8>, String> {
    let data = find_field(particle, "data")
        .and_then(read_str)
        .ok_or("UploadFile requires a string 'data'")?;
    if read_field_bool(particle, "base64").unwrap_or(false) {
        B64.decode(data.trim())
            .map_err(|e| format!("'data' is not valid base64: {e}"))
    } else {
        Ok(data.as_bytes().to_vec())
    }
}

fn opt(particle: &CodeValue, name: &str) -> Option<String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn require_str<'a>(particle: &'a CodeValue, name: &str, class: &str) -> Result<&'a str, String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{class} requires a non-empty string '{name}'"))
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

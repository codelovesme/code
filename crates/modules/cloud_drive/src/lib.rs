//! The `cloud_drive` native module — Google Drive, for the Code programming
//! language, written in Rust on [`code-native`] over `ureq`.
//!
//! The server-side OAuth 2.0 authorization-code flow plus the five file
//! operations an aggregator backend needs: quota, upload, download, list,
//! delete. Secrets live in `Config`, delivered from a manifest — a gene
//! never sees the client secret.
//!
//! The euglena `cloud-drive` organelle this replaces carried stubs for
//! OneDrive and Yandex that only ever returned `ProviderUnavailable`. This
//! module is Google Drive and says so: a `provider` other than `"google"`
//! is an `Exception`, not a silent shrug.
//!
//! Handlers:
//!
//! - `Config { client_id, client_secret, redirect_uri?, scope?, auth_url?,
//!   token_url?, api_base? }` → `ConfigResult { ok }` — the setup particle.
//!   The three URL fields default to Google's real endpoints; override them
//!   for a Google-compatible gateway or a test double.
//! - `AuthUrl { state, redirect_uri?, extra? }` → `AuthUrlResult { url }` —
//!   the URL to send the user to. `extra` is `{ key = "value" }` for extra
//!   query parameters. `BuildAuthUrl` is an alias.
//! - `ExchangeCode { code, redirect_uri? }` → `Tokens { account_email,
//!   access_token, refresh_token, expires_in }`
//! - `RefreshToken { refresh_token }` → `Tokens { … }` (no `account_email`)
//! - `GetQuota { access_token }` → `Quota { account_email, total, used,
//!   available }` — bytes.
//! - `ListFiles { access_token, query?, page_size? }` → `FileList { files,
//!   count }` — `files` is an array of `RemoteFile`.
//! - `UploadFile { access_token, file_name, data, content_type?, base64? }`
//!   → `RemoteFile { file_id, file_name, content_type, size, web_view_url }`
//! - `DownloadFile { access_token, file_id, base64? }` → `FileContent {
//!   file_id, file_name, content_type, data }`
//! - `DeleteFile { access_token, file_id }` → `DeleteResult { existed }`
//!
//! `base64 = true` on `UploadFile`/`DownloadFile` moves bytes the language
//! can't hold directly — `data` is base64 in, base64 out. Without it `data`
//! is the content's UTF-8.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use code_native::*;
use serde_json::Value as Json;
use std::sync::Mutex;
use std::time::Duration;

static CONFIG: Mutex<Option<Config>> = Mutex::new(None);

struct Config {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    scope: String,
    auth_url: String,
    token_url: String,
    api_base: String,
}

const DEFAULT_SCOPE: &str = "openid email profile https://www.googleapis.com/auth/drive.file";
const DEFAULT_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const DEFAULT_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const DEFAULT_API_BASE: &str = "https://www.googleapis.com";
/// Ceiling on a single `DownloadFile` — a language with no way to interrupt
/// itself should not pull an unbounded stream into memory.
const MAX_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024;
const NOT_CONFIGURED: &str = "cloud_drive has no credentials — send Config { … } first";

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
    guarded(&mut *out, "cloud_drive", |out| {
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
            exception(out, "cloud_drive", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Setup + OAuth
// ---------------------------------------------------------------------------

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let cfg = Config {
        client_id: require_str(particle, "client_id", "Config")?.to_string(),
        client_secret: require_str(particle, "client_secret", "Config")?.to_string(),
        redirect_uri: opt_str(particle, "redirect_uri").unwrap_or_default(),
        scope: opt_str(particle, "scope").unwrap_or_else(|| DEFAULT_SCOPE.to_string()),
        auth_url: opt_str(particle, "auth_url").unwrap_or_else(|| DEFAULT_AUTH_URL.to_string()),
        token_url: opt_str(particle, "token_url").unwrap_or_else(|| DEFAULT_TOKEN_URL.to_string()),
        api_base: opt_str(particle, "api_base")
            .unwrap_or_else(|| DEFAULT_API_BASE.to_string())
            .trim_end_matches('/')
            .to_string(),
    };
    *CONFIG.lock().unwrap_or_else(|e| e.into_inner()) = Some(cfg);

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
    let redirect_uri = redirect_uri(particle, cfg)?;

    let mut query = format!(
        "response_type=code&client_id={}&redirect_uri={}&scope={}&state={}\
         &access_type=offline&include_granted_scopes=true&prompt=consent",
        encode(&cfg.client_id),
        encode(redirect_uri),
        encode(&cfg.scope),
        encode(state),
    );
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
    let (token_url, redirect, api_base, id, secret) = {
        let guard = CONFIG.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = guard.as_ref().ok_or(NOT_CONFIGURED)?;
        (
            cfg.token_url.clone(),
            redirect_uri(particle, cfg)?.to_string(),
            cfg.api_base.clone(),
            cfg.client_id.clone(),
            cfg.client_secret.clone(),
        )
    };

    let body = format!(
        "grant_type=authorization_code&code={}&client_id={}&client_secret={}&redirect_uri={}",
        encode(code),
        encode(&id),
        encode(&secret),
        encode(&redirect),
    );
    let token = post_form(&token_url, &body)?;
    let (access, refresh, expires) = read_token(&token)?;

    // Best effort: a token that works but an unreachable userinfo endpoint
    // still yields usable tokens.
    let email = get_json(&format!("{api_base}/oauth2/v3/userinfo"), &access)
        .ok()
        .and_then(|v| v.get("email").and_then(Json::as_str).map(str::to_string))
        .unwrap_or_default();

    tokens(out, &email, &access, &refresh, expires);
    Ok(())
}

fn refresh_token(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let token = require_str(particle, "refresh_token", "RefreshToken")?;
    let (token_url, id, secret) = {
        let guard = CONFIG.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = guard.as_ref().ok_or(NOT_CONFIGURED)?;
        (
            cfg.token_url.clone(),
            cfg.client_id.clone(),
            cfg.client_secret.clone(),
        )
    };

    let body = format!(
        "grant_type=refresh_token&refresh_token={}&client_id={}&client_secret={}",
        encode(token),
        encode(&id),
        encode(&secret),
    );
    let resp = post_form(&token_url, &body)?;
    let (access, refresh, expires) = read_token(&resp)?;
    // Google omits `refresh_token` on a refresh — the old one stays valid.
    let refresh = if refresh.is_empty() {
        token.to_string()
    } else {
        refresh
    };

    tokens(out, "", &access, &refresh, expires);
    Ok(())
}

// ---------------------------------------------------------------------------
// Drive operations
// ---------------------------------------------------------------------------

fn get_quota(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let token = access_token(particle, "GetQuota")?;
    let base = api_base()?;
    let v = get_json(
        &format!("{base}/drive/v3/about?fields=storageQuota,user"),
        &token,
    )?;

    let q = v.get("storageQuota").cloned().unwrap_or(Json::Null);
    let field_u64 = |k: &str| -> f64 {
        q.get(k)
            .and_then(Json::as_str)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0) as f64
    };
    let total = field_u64("limit");
    let used = field_u64("usage");
    let available = (total - used).max(0.0);
    let email = v
        .pointer("/user/emailAddress")
        .and_then(Json::as_str)
        .unwrap_or("")
        .to_string();

    let mut b = SlotBuffer::new(5);
    borrowed_str(b.slot_mut(0), c"Quota");
    owned_str(b.slot_mut(1), &email);
    number(b.slot_mut(2), total);
    number(b.slot_mut(3), used);
    number(b.slot_mut(4), available);
    object(
        out,
        &[c"_class", c"account_email", c"total", c"used", c"available"],
        &mut b,
    );
    b.release_all();
    Ok(())
}

fn list_files(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let token = access_token(particle, "ListFiles")?;
    let base = api_base()?;
    let page_size = find_field(particle, "page_size")
        .and_then(read_number)
        .filter(|n| *n > 0.0)
        .map(|n| n as u64)
        .unwrap_or(100)
        .min(1000);
    let query = opt_str(particle, "query").unwrap_or_else(|| "trashed = false".to_string());

    let url = format!(
        "{base}/drive/v3/files?pageSize={page_size}\
         &fields={}&q={}",
        encode("files(id,name,mimeType,size,webViewLink)"),
        encode(&query),
    );
    let v = get_json(&url, &token)?;
    let files: Vec<Json> = v
        .get("files")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();

    let mut arr = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(files.len());
    for (i, f) in files.iter().enumerate() {
        remote_file(buf.slot_mut(i as i64), f);
    }
    array(&mut arr, &mut buf);
    buf.release_all();

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"FileList");
    copy(b.slot_mut(1), &arr);
    number(b.slot_mut(2), files.len() as f64);
    object(out, &[c"_class", c"files", c"count"], &mut b);
    b.release_all();
    release(&mut arr);
    Ok(())
}

fn upload_file(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let token = access_token(particle, "UploadFile")?;
    let name = require_str(particle, "file_name", "UploadFile")?;
    let bytes = payload(particle, "UploadFile")?;
    let content_type = opt_str(particle, "content_type")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let base = api_base()?;

    let boundary = "codeCloudDriveBoundary7f3a9c";
    let metadata = serde_json::json!({ "name": name }).to_string();
    let mut body: Vec<u8> = Vec::with_capacity(bytes.len() + metadata.len() + 256);
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Type: application/json; charset=UTF-8\r\n\r\n");
    body.extend_from_slice(metadata.as_bytes());
    body.extend_from_slice(format!("\r\n--{boundary}\r\n").as_bytes());
    body.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
    body.extend_from_slice(&bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let url = format!(
        "{base}/upload/drive/v3/files?uploadType=multipart&fields={}",
        encode("id,name,mimeType,size,webViewLink"),
    );
    let mut resp = agent()
        .post(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .header(
            "Content-Type",
            &format!("multipart/related; boundary={boundary}"),
        )
        .send(&body[..])
        .map_err(|e| format!("upload request failed: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("reading the upload response failed: {e}"))?;
    if !(200..300).contains(&status) {
        return Err(format!("upload rejected: HTTP {status}: {}", trim(&text)));
    }
    let v: Json = serde_json::from_str(&text).map_err(|e| format!("parsing the response: {e}"))?;
    remote_file(out, &v);
    Ok(())
}

fn download_file(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let token = access_token(particle, "DownloadFile")?;
    let file_id = require_str(particle, "file_id", "DownloadFile")?;
    let as_base64 = read_field_bool(particle, "base64").unwrap_or(false);
    let base = api_base()?;

    let meta = get_json(
        &format!(
            "{base}/drive/v3/files/{}?fields=id,name,mimeType",
            encode(file_id)
        ),
        &token,
    )?;
    let name = meta
        .get("name")
        .and_then(Json::as_str)
        .unwrap_or("")
        .to_string();
    let content_type = meta
        .get("mimeType")
        .and_then(Json::as_str)
        .unwrap_or("application/octet-stream")
        .to_string();

    let mut resp = agent()
        .get(&format!(
            "{base}/drive/v3/files/{}?alt=media",
            encode(file_id)
        ))
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|e| format!("download request failed: {e}"))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        return Err(format!("download rejected: HTTP {status}: {}", trim(&text)));
    }
    let raw = resp
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD_BYTES)
        .read_to_vec()
        .map_err(|e| match e {
            ureq::Error::BodyExceedsLimit(_) => {
                format!("file exceeds the {MAX_DOWNLOAD_BYTES}-byte download limit")
            }
            other => format!("reading the file failed: {other}"),
        })?;
    let data = if as_base64 {
        B64.encode(&raw)
    } else {
        String::from_utf8_lossy(&raw).into_owned()
    };

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
    let token = access_token(particle, "DeleteFile")?;
    let file_id = require_str(particle, "file_id", "DeleteFile")?;
    let base = api_base()?;

    let mut resp = agent()
        .delete(&format!("{base}/drive/v3/files/{}", encode(file_id)))
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|e| format!("delete request failed: {e}"))?;
    let status = resp.status().as_u16();
    // 404 = already gone: a question, not an error, same as `blob_storage`.
    let existed = status != 404;
    if !existed || (200..300).contains(&status) || status == 204 {
        one_bool(out, c"DeleteResult", c"existed", existed);
        return Ok(());
    }
    let text = resp.body_mut().read_to_string().unwrap_or_default();
    Err(format!("delete rejected: HTTP {status}: {}", trim(&text)))
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

fn get_json(url: &str, token: &str) -> Result<Json, String> {
    let mut resp = agent()
        .get(url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", "application/json")
        .call()
        .map_err(|e| format!("Drive endpoint '{url}' unreachable: {e}"))?;
    finish(resp.status().as_u16(), resp.body_mut().read_to_string())
}

fn finish(status: u16, body: Result<String, ureq::Error>) -> Result<Json, String> {
    let text = body.map_err(|e| format!("reading the response failed: {e}"))?;
    let json: Json =
        serde_json::from_str(&text).map_err(|_| format!("HTTP {status}: {}", trim(&text)))?;
    if status >= 400 {
        let hint = json
            .pointer("/error/message")
            .or_else(|| json.get("error_description"))
            .or_else(|| json.get("error"))
            .and_then(Json::as_str)
            .unwrap_or_else(|| trim(&text));
        return Err(format!("HTTP {status}: {hint}"));
    }
    Ok(json)
}

fn read_token(token: &Json) -> Result<(String, String, f64), String> {
    let access = token
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
    let refresh = token
        .get("refresh_token")
        .and_then(Json::as_str)
        .unwrap_or("")
        .to_string();
    let expires = token
        .get("expires_in")
        .and_then(Json::as_f64)
        .unwrap_or(3600.0);
    Ok((access, refresh, expires))
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

/// A Drive `files` resource as a `RemoteFile` value.
fn remote_file(out: &mut CodeValue, f: &Json) {
    let s = |k: &str| f.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    let file_id = s("id");
    let size = f
        .get("size")
        .and_then(Json::as_str)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0) as f64;
    let view = match f.get("webViewLink").and_then(Json::as_str) {
        Some(v) if !v.is_empty() => v.to_string(),
        _ => format!("https://drive.google.com/file/d/{file_id}/view"),
    };

    let mut b = SlotBuffer::new(6);
    borrowed_str(b.slot_mut(0), c"RemoteFile");
    owned_str(b.slot_mut(1), &file_id);
    owned_str(b.slot_mut(2), &s("name"));
    owned_str(b.slot_mut(3), &s("mimeType"));
    number(b.slot_mut(4), size);
    owned_str(b.slot_mut(5), &view);
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

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

fn api_base() -> Result<String, String> {
    let guard = CONFIG.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .as_ref()
        .map(|c| c.api_base.clone())
        .ok_or(NOT_CONFIGURED.to_string())
}

/// The `access_token` field, rejecting a non-google `provider` while we have
/// the particle — the euglena organelle carried OneDrive/Yandex stubs; this
/// module is Google Drive and does not pretend otherwise.
fn access_token(particle: &CodeValue, class: &str) -> Result<String, String> {
    if let Some(p) = opt_str(particle, "provider") {
        if !p.is_empty() && p != "google" {
            return Err(format!(
                "cloud_drive supports only Google Drive — got provider '{p}'"
            ));
        }
    }
    Ok(require_str(particle, "access_token", class)?.to_string())
}

fn redirect_uri<'a>(particle: &'a CodeValue, cfg: &'a Config) -> Result<&'a str, String> {
    if let Some(v) = find_field(particle, "redirect_uri").and_then(read_str) {
        if !v.is_empty() {
            return Ok(v);
        }
    }
    if !cfg.redirect_uri.is_empty() {
        return Ok(&cfg.redirect_uri);
    }
    Err("redirect_uri is required — set it in Config or pass it on the particle".to_string())
}

fn payload(particle: &CodeValue, class: &str) -> Result<Vec<u8>, String> {
    let data = find_field(particle, "data")
        .and_then(read_str)
        .ok_or_else(|| format!("{class} requires a string 'data'"))?;
    if read_field_bool(particle, "base64").unwrap_or(false) {
        B64.decode(data.trim())
            .map_err(|e| format!("'data' is not valid base64: {e}"))
    } else {
        Ok(data.as_bytes().to_vec())
    }
}

fn opt_str(particle: &CodeValue, name: &str) -> Option<String> {
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

fn trim(s: &str) -> &str {
    let s = s.trim();
    if s.len() > 200 {
        &s[..200]
    } else {
        s
    }
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
        assert_eq!(encode("trashed = false"), "trashed%20%3D%20false");
    }
}

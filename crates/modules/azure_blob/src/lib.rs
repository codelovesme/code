//! The `azure_blob` native module — Azure Blob Storage, for the Code
//! programming language, written in Rust on [`code-native`].
//!
//! `blob_storage` speaks S3, which is what most of the world speaks. Azure
//! is the exception: it has its own REST API and its own SharedKey signing,
//! and no S3 gateway of its own. This is that API, behind **`blob_storage`'s
//! particle contract** — the same classes, the same fields, the same
//! results — so a program moves between the two by changing the module it
//! links and nothing else.
//!
//! That is the whole design rule here. Where Azure's vocabulary differs the
//! contract wins: a container is configured as `bucket`, an object is a
//! `key`, and `Get` on a missing one answers `GetResult { found = false }`
//! rather than an error, because that is what `blob_storage` answers.
//!
//! Handlers:
//!
//! - `Config { bucket, connection_string? | account + key, endpoint?,
//!   create? }` → `ConfigResult { ok }` — the setup particle. A connection
//!   string is the Azure-native way to be told all of it at once and is what
//!   Azurite hands out; `account` and `key` are the same thing spelled out.
//! - `Put { key, data, content_type?, base64? }` → `PutResult { key }`
//! - `Get { key, base64? }` → `GetResult { found, key, data, content_type }`
//! - `Delete { key }` → `DeleteResult { existed }`
//! - `List { prefix? }` → `ListResult { keys, count }`
//!
//! `Upload` and `Download` are accepted for `Put` and `Get`, as they are
//! there.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use code_native::*;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// The REST API this module speaks. Pinned rather than tracking the newest:
/// the signing rules are versioned with it.
const API_VERSION: &str = "2021-08-06";

struct Account {
    name: String,
    key: Vec<u8>,
    /// No trailing slash. For Azurite this carries the account in its path
    /// (`http://host:10000/devstoreaccount1`), which the canonical resource
    /// has to repeat — see `canonical_resource`.
    endpoint: String,
    container: String,
}

static ACCOUNT: Mutex<Option<Account>> = Mutex::new(None);

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
    guarded(&mut *out, "azure_blob", |out| {
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
            exception(out, "azure_blob", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let container = require_str(particle, "bucket", "Config")?.to_string();

    let (name, key_text, endpoint) = match optional(particle, "connection_string") {
        Some(cs) => parse_connection_string(cs)?,
        None => {
            let name = require_str(particle, "account", "Config")?.to_string();
            let key = require_str(particle, "key", "Config")?.to_string();
            // Without an endpoint it is the real Azure, whose host is the
            // account's own name.
            let endpoint = optional(particle, "endpoint")
                .map(|e| e.trim_end_matches('/').to_string())
                .unwrap_or_else(|| format!("https://{name}.blob.core.windows.net"));
            (name, key, endpoint)
        }
    };

    let key = B64
        .decode(key_text.trim())
        .map_err(|e| format!("the account key is not valid base64: {e}"))?;

    let account = Account {
        name,
        key,
        endpoint,
        container,
    };

    // `create = true` makes the container if it isn't there. Off by default,
    // for the reason `blob_storage` gives: connecting to storage is not the
    // same as making somewhere to put things.
    if read_field_bool(particle, "create").unwrap_or(false) {
        create_container(&account)?;
    }

    *ACCOUNT.lock().unwrap_or_else(|e| e.into_inner()) = Some(account);
    one_bool(out, c"ConfigResult", c"ok", true);
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
    let content_type = optional(particle, "content_type").unwrap_or("application/octet-stream");

    let account = account()?;
    let path = blob_path(&account, key);
    // `x-ms-blob-type` is signed because it is sent; the signature has to
    // cover exactly the `x-ms-*` headers on the request and no others.
    let extra = [("x-ms-blob-type", "BlockBlob")];
    let (date, auth) = authorize(
        &account,
        "PUT",
        &path,
        &[],
        bytes.len(),
        content_type,
        &extra,
    )?;

    let response = ureq::put(&format!("{}{}", account.endpoint, path))
        .config()
        .timeout_global(Some(Duration::from_secs(60)))
        .http_status_as_error(false)
        .build()
        .header("x-ms-date", &date)
        .header("x-ms-version", API_VERSION)
        .header("x-ms-blob-type", "BlockBlob")
        .header("Content-Type", content_type)
        .header("Authorization", &auth)
        .send(&bytes[..])
        .map_err(|e| format!("Put failed: {e}"))?;

    check(response, "Put")?;
    one_str(out, c"PutResult", c"key", key);
    Ok(())
}

fn get(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Get")?;
    let as_base64 = read_field_bool(particle, "base64").unwrap_or(false);

    let account = account()?;
    let path = blob_path(&account, key);
    let (date, auth) = authorize(&account, "GET", &path, &[], 0, "", &[])?;

    let mut response = ureq::get(&format!("{}{}", account.endpoint, path))
        .config()
        .timeout_global(Some(Duration::from_secs(60)))
        .http_status_as_error(false)
        .build()
        .header("x-ms-date", &date)
        .header("x-ms-version", API_VERSION)
        .header("Authorization", &auth)
        .call()
        .map_err(|e| format!("Get failed: {e}"))?;

    let status = response.status().as_u16();
    // "Is it there?" is a question, not an error — the same answer
    // `blob_storage` gives.
    if status == 404 {
        return not_found(out, key);
    }
    if !(200..300).contains(&status) {
        return Err(format!(
            "Get failed: HTTP {status}: {}",
            body_of(&mut response)
        ));
    }

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = response
        .body_mut()
        .read_to_vec()
        .map_err(|e| format!("Get failed while reading the body: {e}"))?;
    let data = if as_base64 {
        B64.encode(&bytes)
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };

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

    let account = account()?;
    let path = blob_path(&account, key);
    let (date, auth) = authorize(&account, "DELETE", &path, &[], 0, "", &[])?;

    let mut response = ureq::delete(&format!("{}{}", account.endpoint, path))
        .config()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build()
        .header("x-ms-date", &date)
        .header("x-ms-version", API_VERSION)
        .header("Authorization", &auth)
        .call()
        .map_err(|e| format!("Delete failed: {e}"))?;

    let status = response.status().as_u16();
    // Deleting what is not there is not a failure; it is the answer `false`.
    // One round trip rather than a look followed by a delete, which would
    // also be two chances for somebody else to get in between.
    let existed = match status {
        404 => false,
        s if (200..300).contains(&s) => true,
        s => {
            return Err(format!(
                "Delete failed: HTTP {s}: {}",
                body_of(&mut response)
            ))
        }
    };
    one_bool(out, c"DeleteResult", c"existed", existed);
    Ok(())
}

fn list(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let prefix = optional(particle, "prefix").unwrap_or("").to_string();

    let account = account()?;
    let path = format!("/{}", account.container);
    // Signed query parameters, sorted by name — the canonical resource wants
    // them in order, and `authorize` is what sorts them.
    let mut query: Vec<(String, String)> = vec![
        ("comp".to_string(), "list".to_string()),
        ("restype".to_string(), "container".to_string()),
    ];
    if !prefix.is_empty() {
        query.push(("prefix".to_string(), prefix.clone()));
    }
    let (date, auth) = authorize(&account, "GET", &path, &query, 0, "", &[])?;

    let url = format!(
        "{}{}?restype=container&comp=list{}",
        account.endpoint,
        path,
        if prefix.is_empty() {
            String::new()
        } else {
            format!("&prefix={}", encode(&prefix))
        }
    );
    let mut response = ureq::get(&url)
        .config()
        .timeout_global(Some(Duration::from_secs(60)))
        .http_status_as_error(false)
        .build()
        .header("x-ms-date", &date)
        .header("x-ms-version", API_VERSION)
        .header("Authorization", &auth)
        .call()
        .map_err(|e| format!("List failed: {e}"))?;

    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!(
            "List failed: HTTP {status}: {}",
            body_of(&mut response)
        ));
    }
    let xml = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("List failed while reading the body: {e}"))?;

    let mut keys = blob_names(&xml);
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
// SharedKey
// ---------------------------------------------------------------------------

/// The date and the `Authorization` header for one request.
///
/// Azure signs a fixed list of HTTP headers — most of them empty for what
/// this module sends — then every `x-ms-*` header, then the resource. The
/// order and the blank lines are the format; a wrong one is a 403 that says
/// nothing about which line was wrong.
fn authorize(
    account: &Account,
    method: &str,
    path: &str,
    query: &[(String, String)],
    content_length: usize,
    content_type: &str,
    extra: &[(&str, &str)],
) -> Result<(String, String), String> {
    let date = httpdate::fmt_http_date(SystemTime::now());

    let mut headers: Vec<(String, String)> = extra
        .iter()
        .map(|(k, v)| (k.to_lowercase(), v.to_string()))
        .collect();
    headers.push(("x-ms-date".to_string(), date.clone()));
    headers.push(("x-ms-version".to_string(), API_VERSION.to_string()));
    headers.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical_headers = headers
        .iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join("\n");

    // A zero length is sent as an empty line, not as "0".
    let length = if content_length > 0 {
        content_length.to_string()
    } else {
        String::new()
    };

    // VERB, then Content-Encoding, Content-Language, Content-Length,
    // Content-MD5, Content-Type, Date, If-Modified-Since, If-Match,
    // If-None-Match, If-Unmodified-Since, Range. `Date` is empty because
    // `x-ms-date` carries it, and it wins when both are present.
    let string_to_sign = format!(
        "{method}\n\n\n{length}\n\n{content_type}\n\n\n\n\n\n\n{canonical_headers}\n{resource}",
        resource = canonical_resource(account, path, query),
    );

    let mut mac = Hmac::<Sha256>::new_from_slice(&account.key)
        .map_err(|e| format!("the account key cannot sign: {e}"))?;
    mac.update(string_to_sign.as_bytes());
    let signature = B64.encode(mac.finalize().into_bytes());

    Ok((date, format!("SharedKey {}:{}", account.name, signature)))
}

/// `/{account}{endpoint path}{path}`, then each signed query parameter on
/// its own line, sorted by name.
///
/// The endpoint's own path is in there because Azurite puts the account in
/// the URL (`http://host:10000/devstoreaccount1/…`) rather than in the
/// hostname, and Azure's rule is the account name followed by the *full*
/// path of the request. So against Azurite the account appears twice, which
/// looks wrong and is what the SDKs compute.
fn canonical_resource(account: &Account, path: &str, query: &[(String, String)]) -> String {
    let mut resource = format!(
        "/{}{}{}",
        account.name,
        endpoint_path(&account.endpoint),
        path
    );
    let mut sorted: Vec<&(String, String)> = query.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, value) in sorted {
        resource.push('\n');
        resource.push_str(&name.to_lowercase());
        resource.push(':');
        resource.push_str(value);
    }
    resource
}

/// The path component of a URL: everything after the host, without a
/// trailing slash. `https://acct.blob.core.windows.net` has none.
fn endpoint_path(endpoint: &str) -> String {
    let after_scheme = endpoint
        .find("://")
        .map(|i| &endpoint[i + 3..])
        .unwrap_or(endpoint);
    match after_scheme.find('/') {
        Some(i) => after_scheme[i..].trim_end_matches('/').to_string(),
        None => String::new(),
    }
}

/// `AccountName`, `AccountKey` and `BlobEndpoint` out of the string Azure
/// and Azurite hand out. Split on the first `=` only: a key ends in base64
/// padding and would otherwise lose it.
fn parse_connection_string(cs: &str) -> Result<(String, String, String), String> {
    let (mut name, mut key, mut endpoint, mut protocol) =
        (String::new(), String::new(), String::new(), String::new());
    for part in cs.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part.split_once('=') {
            Some(("AccountName", v)) => name = v.to_string(),
            Some(("AccountKey", v)) => key = v.to_string(),
            Some(("BlobEndpoint", v)) => endpoint = v.to_string(),
            Some(("DefaultEndpointsProtocol", v)) => protocol = v.to_string(),
            _ => {}
        }
    }
    if name.is_empty() || key.is_empty() {
        return Err("the connection string needs an AccountName and an AccountKey".to_string());
    }
    if endpoint.is_empty() {
        let protocol = if protocol.is_empty() {
            "https"
        } else {
            &protocol
        };
        endpoint = format!("{protocol}://{name}.blob.core.windows.net");
    }
    Ok((name, key, endpoint.trim_end_matches('/').to_string()))
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

fn create_container(account: &Account) -> Result<(), String> {
    let path = format!("/{}", account.container);
    let query = [("restype".to_string(), "container".to_string())];
    let (date, auth) = authorize(account, "PUT", &path, &query, 0, "", &[])?;

    let mut response = ureq::put(&format!("{}{}?restype=container", account.endpoint, path))
        .config()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build()
        .header("x-ms-date", &date)
        .header("x-ms-version", API_VERSION)
        .header("Authorization", &auth)
        .send_empty()
        .map_err(|e| format!("cannot create container '{}': {e}", account.container))?;

    let status = response.status().as_u16();
    // 409 is "it already exists", which is the outcome that was wanted.
    if status == 409 || (200..300).contains(&status) {
        return Ok(());
    }
    Err(format!(
        "cannot create container '{}': HTTP {status}: {}",
        account.container,
        body_of(&mut response)
    ))
}

/// Every `<Name>` inside the `<Blobs>` element of a list response.
///
/// Read by hand rather than with an XML crate: the document has one shape,
/// this module asks for no delimiter so there are no `<BlobPrefix>` entries
/// to tell apart, and a parser would be the largest dependency here by far.
fn blob_names(xml: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<Name>") {
        let after = &rest[start + "<Name>".len()..];
        match after.find("</Name>") {
            Some(end) => {
                names.push(unescape(&after[..end]));
                rest = &after[end..];
            }
            None => break,
        }
    }
    names
}

fn unescape(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// Percent-encode what may not travel in a query value as it stands.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn blob_path(account: &Account, key: &str) -> String {
    format!("/{}/{}", account.container, key.trim_start_matches('/'))
}

fn account() -> Result<Account, String> {
    let guard = ACCOUNT.lock().unwrap_or_else(|e| e.into_inner());
    let held = guard
        .as_ref()
        .ok_or("azure_blob is not configured — send Config { bucket, … } first")?;
    Ok(Account {
        name: held.name.clone(),
        key: held.key.clone(),
        endpoint: held.endpoint.clone(),
        container: held.container.clone(),
    })
}

fn check(mut response: ureq::http::Response<ureq::Body>, op: &str) -> Result<(), String> {
    let status = response.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(());
    }
    Err(format!(
        "{op} failed: HTTP {status}: {}",
        body_of(&mut response)
    ))
}

/// Azure explains a refusal in the body; passing it through is the
/// difference between a status nobody can act on and a reason.
fn body_of(response: &mut ureq::http::Response<ureq::Body>) -> String {
    let text = response
        .body_mut()
        .read_to_string()
        .unwrap_or_else(|_| String::new());
    let trimmed = text.trim();
    if trimmed.len() > 400 {
        format!("{}…", &trimmed[..400])
    } else {
        trimmed.to_string()
    }
}

fn optional<'a>(particle: &'a CodeValue, name: &str) -> Option<&'a str> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
}

fn require_str<'a>(particle: &'a CodeValue, name: &str, class: &str) -> Result<&'a str, String> {
    optional(particle, name).ok_or_else(|| format!("{class} requires a non-empty string '{name}'"))
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

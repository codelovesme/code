//! The `blob_fs` native module — `blob_storage`'s handlers, kept as plain
//! files on a disk, for the Code programming language, written in Rust on
//! [`code-native`].
//!
//! Same particles and result shapes as `blob_storage`, `azure_blob` and
//! `blob_storage_mock` — `Config`, `Put`/`Upload`, `Get`/`Download`,
//! `Delete`, `List` — so a program moves to it by changing the module its
//! manifest names and the `Config` it sends, and nothing else.
//!
//! Every blob is one ordinary file, named by its key, beside a small JSON
//! file that says what it is:
//!
//! ```text
//! <root>/data/<bucket>/<key>          the bytes, exactly as they were put
//! <root>/meta/<bucket>/<key>.json     { content_type, size, sha256, archived }
//! <root>/tmp/                         writes in progress, never a blob
//! ```
//!
//! **Why plain files.** Azurite keeps blobs as extents with generated names
//! and an index only it can read; lose the index and the bytes are anonymous.
//! Here a person can open the folder and find a photo by its path, a backup is
//! a copy, and an archive disk is a directory tree.
//!
//! Handlers:
//!
//! - `Config { bucket, root, create? }` → `ConfigResult { ok }` — `create =
//!   true` makes the bucket's folders; otherwise they must exist.
//! - `Put { key, data, content_type?, base64? }` → `PutResult { key }` — atomic:
//!   written to `tmp/`, synced, then renamed into place, so a reader sees the
//!   old file or the whole new one, and a write killed half-way leaves no
//!   file at all. `sha256` and `size` go into the meta.
//! - `Get { key, base64? }` → `GetResult { found, key, data, content_type }`;
//!   or `Archived { key, disks }` when the meta says the file was moved to
//!   archive disks and no copy is left here — "it is on A-007" is not "not
//!   found".
//! - `Delete { key }` → `DeleteResult { existed }` — the file and its meta;
//!   folders left empty are removed.
//! - `List { prefix? }` → `ListResult { keys, count }` — sorted, archived ones
//!   included.
//!
//! A key is a relative path: `/`-separated, no empty part, no `.` or `..`, no
//! backslash or NUL, and not absolute. Anything else is an `Exception` — a key
//! is never quietly rewritten, so two keys cannot land on one file, and none
//! can reach outside its bucket. A bucket is one such part.
//!
//! Built so `Move`/`Rename` can be added for Drive without changing what is
//! here: a key is only ever turned into a path by `paths`, and a file's
//! identity is its meta, which moves with it.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use code_native::*;
use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

struct Place {
    /// `<root>/data/<bucket>`
    data: PathBuf,
    /// `<root>/meta/<bucket>`
    meta: PathBuf,
    /// `<root>/tmp`
    tmp: PathBuf,
}

static PLACE: Mutex<Option<Place>> = Mutex::new(None);
static WRITES: AtomicU64 = AtomicU64::new(0);

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
    guarded(&mut *out, "blob_fs", |out| {
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
            exception(out, "blob_fs", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let bucket = require_str(particle, "bucket", "Config")?;
    check_part(bucket, "bucket")?;
    let root = PathBuf::from(require_str(particle, "root", "Config")?);
    if !root.is_absolute() {
        return Err(format!(
            "Config 'root' must be an absolute path, not '{}'",
            root.display()
        ));
    }
    let create = read_field_bool(particle, "create").unwrap_or(false);

    let place = Place {
        data: root.join("data").join(bucket),
        meta: root.join("meta").join(bucket),
        tmp: root.join("tmp"),
    };
    for dir in [&place.data, &place.meta, &place.tmp] {
        if create {
            fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create '{}': {e}", dir.display()))?;
        } else if !dir.is_dir() {
            return Err(format!(
                "'{}' does not exist — make it, or send Config {{ create = true }}",
                dir.display()
            ));
        }
    }
    *PLACE.lock().unwrap_or_else(|e| e.into_inner()) = Some(place);

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

    let (file, meta, tmp) = paths(&key)?;
    let sha256 = hex(&Sha256::digest(&bytes));
    let about = json!({
        "content_type": content_type,
        "size": bytes.len(),
        "sha256": sha256,
        "archived": false,
    });
    let about =
        serde_json::to_vec_pretty(&about).map_err(|e| format!("cannot describe '{key}': {e}"))?;

    // The bytes first, then what they are: a meta never describes a file that
    // is not there yet. A crash between the two leaves a file without meta,
    // which `Get` still serves.
    commit(&tmp, &file, &bytes).map_err(|e| format!("cannot write '{key}': {e}"))?;
    commit(&tmp, &meta, &about).map_err(|e| format!("cannot describe '{key}': {e}"))?;

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
    let (file, meta, _) = paths(key)?;
    let about = read_meta(&meta).map_err(|e| format!("cannot read what '{key}' is: {e}"))?;

    let bytes = match fs::read(&file) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("cannot read '{key}': {e}")),
    };

    if bytes.is_none() {
        if let Some(about) = &about {
            if about.get("archived").and_then(Json::as_bool) == Some(true) {
                let disks: Vec<String> = about
                    .get("disks")
                    .and_then(Json::as_array)
                    .map(|d| {
                        d.iter()
                            .filter_map(Json::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                let mut list = CodeValue::zeroed();
                strings(&mut list, &disks);
                let mut b = SlotBuffer::new(3);
                borrowed_str(b.slot_mut(0), c"Archived");
                owned_str(b.slot_mut(1), key);
                copy(b.slot_mut(2), &list);
                object(out, &[c"_class", c"key", c"disks"], &mut b);
                b.release_all();
                release(&mut list);
                return Ok(());
            }
        }
    }

    let mut b = SlotBuffer::new(5);
    borrowed_str(b.slot_mut(0), c"GetResult");
    owned_str(b.slot_mut(2), key);
    match bytes {
        Some(bytes) => {
            let data = if as_base64 {
                B64.encode(&bytes)
            } else {
                String::from_utf8_lossy(&bytes).into_owned()
            };
            let content_type = about
                .as_ref()
                .and_then(|a| a.get("content_type"))
                .and_then(Json::as_str)
                .unwrap_or("application/octet-stream");
            boolean(b.slot_mut(1), true);
            owned_str(b.slot_mut(3), &data);
            owned_str(b.slot_mut(4), content_type);
        }
        None => {
            boolean(b.slot_mut(1), false);
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
    let (file, meta, _) = paths(key)?;
    let (data_root, meta_root) = roots()?;
    let had_file = remove(&file).map_err(|e| format!("cannot delete '{key}': {e}"))?;
    let had_meta = remove(&meta).map_err(|e| format!("cannot delete what '{key}' is: {e}"))?;
    prune(&file, &data_root);
    prune(&meta, &meta_root);

    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"DeleteResult");
    boolean(b.slot_mut(1), had_file || had_meta);
    object(out, &[c"_class", c"existed"], &mut b);
    b.release_all();
    Ok(())
}

fn list(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let prefix = find_field(particle, "prefix")
        .and_then(read_str)
        .unwrap_or("")
        .to_string();
    let (data_root, meta_root) = roots()?;

    // The data tree, and the meta tree for files that live only on archive
    // disks now. A file someone copied in by hand has no meta and still
    // counts.
    let mut keys = BTreeSet::new();
    walk(&data_root, &data_root, &mut |k| {
        keys.insert(k);
    })
    .map_err(|e| format!("cannot list: {e}"))?;
    walk(&meta_root, &meta_root, &mut |k| {
        if let Some(k) = k.strip_suffix(".json") {
            keys.insert(k.to_string());
        }
    })
    .map_err(|e| format!("cannot list: {e}"))?;
    let keys: Vec<String> = keys
        .into_iter()
        .filter(|k| k.starts_with(&prefix))
        .collect();

    let mut arr = CodeValue::zeroed();
    strings(&mut arr, &keys);
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
// Keys and paths
// ---------------------------------------------------------------------------

const NOT_CONFIGURED: &str = "blob_fs is not configured — send Config { bucket, root } first";

/// One part of a key, or a bucket: something that is a file or folder name
/// and nothing more.
fn check_part(part: &str, what: &str) -> Result<(), String> {
    if part.is_empty() || part == "." || part == ".." {
        return Err(format!("'{part}' is not a usable {what} name"));
    }
    if part.contains(['/', '\\', '\0']) {
        return Err(format!(
            "{what} name '{part}' may not contain '/', '\\' or NUL"
        ));
    }
    Ok(())
}

/// The key's file, its meta, and where its writes are staged. `Err` for a
/// key that is not a plain relative path, or before `Config`.
fn paths(key: &str) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    if key.starts_with('/') {
        return Err(format!(
            "key '{key}' is absolute — keys are relative to the bucket"
        ));
    }
    for part in key.split('/') {
        check_part(part, "key part").map_err(|e| format!("key '{key}': {e}"))?;
    }
    let guard = PLACE.lock().unwrap_or_else(|e| e.into_inner());
    let place = guard.as_ref().ok_or(NOT_CONFIGURED)?;
    Ok((
        place.data.join(key),
        place.meta.join(format!("{key}.json")),
        place.tmp.clone(),
    ))
}

fn roots() -> Result<(PathBuf, PathBuf), String> {
    let guard = PLACE.lock().unwrap_or_else(|e| e.into_inner());
    let place = guard.as_ref().ok_or(NOT_CONFIGURED)?;
    Ok((place.data.clone(), place.meta.clone()))
}

// ---------------------------------------------------------------------------
// The disk
// ---------------------------------------------------------------------------

/// Write `bytes` to `dest` so that no reader ever sees half of it: staged in
/// `tmp`, synced, renamed over `dest`, and the folder synced so the rename
/// survives a power cut.
fn commit(tmp: &Path, dest: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = dest
        .parent()
        .ok_or_else(|| io::Error::other("no parent folder"))?;
    fs::create_dir_all(parent)?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let staged = tmp.join(format!(
        "{}-{}-{}.part",
        std::process::id(),
        WRITES.fetch_add(1, Ordering::Relaxed),
        nanos
    ));
    let written = (|| {
        let mut f = File::create(&staged)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&staged, dest)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&staged);
    }
    written?;
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

fn read_meta(path: &Path) -> Result<Option<Json>, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| e.to_string()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn remove(path: &Path) -> io::Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Remove the folders a delete left empty, up to (not including) the
/// bucket's own. Best effort: a folder that is not empty simply stays.
fn prune(path: &Path, stop: &Path) {
    let mut at = path.parent();
    while let Some(dir) = at {
        if dir == stop || !dir.starts_with(stop) || fs::remove_dir(dir).is_err() {
            break;
        }
        at = dir.parent();
    }
}

/// Every file under `dir`, as a `/`-separated path relative to `base`.
fn walk(base: &Path, dir: &Path, found: &mut dyn FnMut(String)) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            walk(base, &path, found)?;
        } else if kind.is_file() {
            if let Ok(rel) = path.strip_prefix(base) {
                let parts: Vec<String> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                found(parts.join("/"));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn strings(out: &mut CodeValue, items: &[String]) {
    let mut buf = SlotBuffer::new(items.len());
    for (i, s) in items.iter().enumerate() {
        owned_str(buf.slot_mut(i as i64), s);
    }
    array(out, &mut buf);
    buf.release_all();
}

fn require_str<'a>(particle: &'a CodeValue, name: &str, class: &str) -> Result<&'a str, String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{class} requires a non-empty string '{name}'"))
}

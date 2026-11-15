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
//! - `Get { key, start, length }` — the same answer for that byte range only,
//!   read without the rest of the file.
//! - `Move { key, dest }` → `MoveResult { key }` — a rename on the disk, meta
//!   and all; nothing is read. A file already at `dest` is refused.
//! - `Hash { key }` → `HashResult { key, found, size, sha256 }` — from the
//!   meta, or read through once for a file that has none.
//! - `Locate { key }` → `LocateResult { key, found, path }` — the file's
//!   absolute path on this machine, for a program that hands a large file to
//!   something that reads files (a web server's byte ranges, ffmpeg).
//!
//! A file too large to cross as one particle arrives in parts:
//!
//! - `PutPart { key, offset, data, base64?, sha256? }` → `PartResult { key,
//!   received }` — writes the bytes at `offset` of the key's staged file. An
//!   offset past what has arrived is refused (no gaps); one before it writes
//!   again, so a part sent twice after a lost answer does no harm. `sha256`,
//!   when given, is checked against the part's bytes first.
//! - `Received { key }` → `ReceivedResult { key, received }` — how many bytes
//!   have arrived, so an upload cut off goes on from there.
//! - `Seal { key, size?, sha256?, content_type? }` → `SealResult { key, size,
//!   sha256 }` — reads the staged file through once for its sha256, checks
//!   `size` and `sha256` when given, and moves it into place like a `Put`.
//!   A file that does not match is thrown away, so the next try starts clean.
//! - `Abandon { key }` → `AbandonResult { existed }` — throws a staged file
//!   away.
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
            "Move" => move_blob(out, particle),
            "Hash" => hash(out, particle),
            "Locate" => locate(out, particle),
            "PutPart" => put_part(out, particle),
            "Received" => received(out, particle),
            "Seal" => seal(out, particle),
            "Abandon" => abandon(out, particle),
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

    let range = match (
        find_field(particle, "start"),
        find_field(particle, "length"),
    ) {
        (None, None) => None,
        (start, length) => {
            let start = start.and_then(read_number).unwrap_or(0.0);
            let length = length.and_then(read_number).unwrap_or(f64::MAX);
            if start.is_nan() || length.is_nan() || start < 0.0 || length < 0.0 {
                return Err(format!(
                    "Get '{key}': 'start' and 'length' must be numbers ≥ 0"
                ));
            }
            Some((start as u64, length.min(u64::MAX as f64) as u64))
        }
    };
    let read = match range {
        None => fs::read(&file),
        Some((start, length)) => read_range(&file, start, length),
    };
    let bytes = match read {
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

fn move_blob(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Move")?.to_string();
    let dest = require_str(particle, "dest", "Move")?.to_string();
    let (file, meta, _) = paths(&key)?;
    let (to_file, to_meta, _) = paths(&dest)?;
    let (data_root, meta_root) = roots()?;
    if key != dest {
        if !file.is_file() {
            return Err(format!("Move: there is no '{key}' to move"));
        }
        if to_file.exists() {
            return Err(format!("Move: '{dest}' is taken"));
        }
        let parent = to_file.parent().ok_or("no parent folder")?;
        fs::create_dir_all(parent).map_err(|e| format!("cannot make room for '{dest}': {e}"))?;
        fs::rename(&file, &to_file).map_err(|e| format!("cannot move '{key}' to '{dest}': {e}"))?;
        if meta.is_file() {
            let parent = to_meta.parent().ok_or("no parent folder")?;
            fs::create_dir_all(parent).map_err(|e| format!("cannot describe '{dest}': {e}"))?;
            fs::rename(&meta, &to_meta).map_err(|e| format!("cannot describe '{dest}': {e}"))?;
        }
        prune(&file, &data_root);
        prune(&meta, &meta_root);
    }
    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"MoveResult");
    owned_str(b.slot_mut(1), &dest);
    object(out, &[c"_class", c"key"], &mut b);
    b.release_all();
    Ok(())
}

fn hash(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Hash")?;
    let (file, meta, _) = paths(key)?;
    let about = read_meta(&meta).map_err(|e| format!("cannot read what '{key}' is: {e}"))?;
    let known = about.as_ref().and_then(|a| {
        Some((
            a.get("size")?.as_u64()?,
            a.get("sha256")?.as_str()?.to_string(),
        ))
    });
    let found = file.is_file();
    let (size, sha256) = match (found, known) {
        (false, _) => (0, String::new()),
        (true, Some(known)) => known,
        (true, None) => hash_file(&file).map_err(|e| format!("cannot read '{key}': {e}"))?,
    };
    let mut b = SlotBuffer::new(5);
    borrowed_str(b.slot_mut(0), c"HashResult");
    owned_str(b.slot_mut(1), key);
    boolean(b.slot_mut(2), found);
    number(b.slot_mut(3), size as f64);
    owned_str(b.slot_mut(4), &sha256);
    object(
        out,
        &[c"_class", c"key", c"found", c"size", c"sha256"],
        &mut b,
    );
    b.release_all();
    Ok(())
}

fn locate(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Locate")?;
    let (file, _, _) = paths(key)?;
    let mut b = SlotBuffer::new(4);
    borrowed_str(b.slot_mut(0), c"LocateResult");
    owned_str(b.slot_mut(1), key);
    boolean(b.slot_mut(2), file.is_file());
    owned_str(b.slot_mut(3), &file.to_string_lossy());
    object(out, &[c"_class", c"key", c"found", c"path"], &mut b);
    b.release_all();
    Ok(())
}

fn put_part(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "PutPart")?.to_string();
    let offset = find_field(particle, "offset")
        .and_then(read_number)
        .filter(|n| *n >= 0.0 && n.fract() == 0.0)
        .ok_or("PutPart requires a whole number 'offset' ≥ 0")? as u64;
    let data = find_field(particle, "data")
        .and_then(read_str)
        .ok_or("PutPart requires a string 'data'")?;
    let bytes = if read_field_bool(particle, "base64").unwrap_or(false) {
        B64.decode(data.trim())
            .map_err(|e| format!("'data' is not valid base64: {e}"))?
    } else {
        data.as_bytes().to_vec()
    };
    if let Some(want) = find_field(particle, "sha256")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
    {
        let got = hex(&Sha256::digest(&bytes));
        if !got.eq_ignore_ascii_case(want) {
            return Err(format!(
                "PutPart '{key}' at {offset}: the part's sha256 is {got}, not {want} — it was damaged on the way; send it again"
            ));
        }
    }
    paths(&key)?;
    let staged = staged_path(&key)?;
    let parent = staged.parent().ok_or("no parent folder")?;
    fs::create_dir_all(parent).map_err(|e| format!("cannot stage '{key}': {e}"))?;
    let have = fs::metadata(&staged).map(|m| m.len()).unwrap_or(0);
    if offset > have {
        return Err(format!(
            "PutPart '{key}': offset {offset} is past the {have} bytes received — send from {have}"
        ));
    }
    let received = (|| -> io::Result<u64> {
        use std::io::{Seek, SeekFrom};
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&staged)?;
        f.seek(SeekFrom::Start(offset))?;
        f.write_all(&bytes)?;
        f.sync_data()?;
        Ok(f.metadata()?.len())
    })()
    .map_err(|e| format!("cannot write part of '{key}': {e}"))?;
    part_answer(out, c"PartResult", &key, received);
    Ok(())
}

fn received(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Received")?.to_string();
    paths(&key)?;
    let have = fs::metadata(staged_path(&key)?)
        .map(|m| m.len())
        .unwrap_or(0);
    part_answer(out, c"ReceivedResult", &key, have);
    Ok(())
}

fn part_answer(out: &mut CodeValue, class: &'static std::ffi::CStr, key: &str, received: u64) {
    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), class);
    owned_str(b.slot_mut(1), key);
    number(b.slot_mut(2), received as f64);
    object(out, &[c"_class", c"key", c"received"], &mut b);
    b.release_all();
}

fn seal(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Seal")?.to_string();
    let (file, meta, tmp) = paths(&key)?;
    let staged = staged_path(&key)?;
    if !staged.is_file() {
        return Err(format!("Seal: nothing of '{key}' has arrived"));
    }
    let (size, sha256) = hash_file(&staged).map_err(|e| format!("cannot read '{key}': {e}"))?;
    let want_size = find_field(particle, "size").and_then(read_number);
    let want_sha = find_field(particle, "sha256")
        .and_then(read_str)
        .filter(|s| !s.is_empty());
    let wrong = match (want_size, want_sha) {
        (Some(n), _) if n as u64 != size => Some(format!("{size} bytes arrived, not {n}")),
        (_, Some(w)) if !w.eq_ignore_ascii_case(&sha256) => {
            Some(format!("its sha256 is {sha256}, not {w}"))
        }
        _ => None,
    };
    if let Some(wrong) = wrong {
        let _ = fs::remove_file(&staged);
        return Err(format!(
            "Seal '{key}': {wrong} — the file was thrown away; upload it again"
        ));
    }
    let content_type = find_field(particle, "content_type")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("application/octet-stream")
        .to_string();
    let about =
        json!({ "content_type": content_type, "size": size, "sha256": sha256, "archived": false });
    let about =
        serde_json::to_vec_pretty(&about).map_err(|e| format!("cannot describe '{key}': {e}"))?;
    let parent = file.parent().ok_or("no parent folder")?;
    fs::create_dir_all(parent).map_err(|e| format!("cannot write '{key}': {e}"))?;
    fs::rename(&staged, &file).map_err(|e| format!("cannot put '{key}' in place: {e}"))?;
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    commit(&tmp, &meta, &about).map_err(|e| format!("cannot describe '{key}': {e}"))?;
    let mut b = SlotBuffer::new(4);
    borrowed_str(b.slot_mut(0), c"SealResult");
    owned_str(b.slot_mut(1), &key);
    number(b.slot_mut(2), size as f64);
    owned_str(b.slot_mut(3), &sha256);
    object(out, &[c"_class", c"key", c"size", c"sha256"], &mut b);
    b.release_all();
    Ok(())
}

fn abandon(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Abandon")?.to_string();
    paths(&key)?;
    let existed =
        remove(&staged_path(&key)?).map_err(|e| format!("cannot throw away '{key}': {e}"))?;
    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"AbandonResult");
    boolean(b.slot_mut(1), existed);
    object(out, &[c"_class", c"existed"], &mut b);
    b.release_all();
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

/// Where a key's parts gather until `Seal`: `<root>/tmp/parts/<bucket>/`, by
/// the key's sha256 so no key can name a path there. Inside `tmp/`, so a
/// staged file is never a blob.
fn staged_path(key: &str) -> Result<PathBuf, String> {
    let guard = PLACE.lock().unwrap_or_else(|e| e.into_inner());
    let place = guard.as_ref().ok_or(NOT_CONFIGURED)?;
    let bucket = place
        .data
        .file_name()
        .map(|b| b.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(place
        .tmp
        .join("parts")
        .join(bucket)
        .join(hex(&Sha256::digest(key.as_bytes()))))
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

/// `length` bytes from `start`, or fewer at the end of the file.
fn read_range(path: &Path, start: u64, length: u64) -> io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = File::open(path)?;
    let size = f.metadata()?.len();
    let start = start.min(size);
    let take = length.min(size - start);
    f.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity(take as usize);
    f.take(take).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Size and sha256 of a file, read through in 1 MB pieces.
fn hash_file(path: &Path) -> io::Result<(u64, String)> {
    use std::io::Read;
    let mut f = File::open(path)?;
    let mut digest = Sha256::new();
    let mut piece = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = f.read(&mut piece)?;
        if n == 0 {
            break;
        }
        digest.update(&piece[..n]);
        size += n as u64;
    }
    Ok((size, hex(&digest.finalize())))
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

//! The `fs` native module — files and directories under a sandboxed base,
//! for the Code programming language, written in Rust on [`code-native`].
//!
//! Every path a handler is given is resolved **inside** the base directory
//! set by `Config`. A leading `/` is treated as base-relative, and a `..` that
//! would climb above the base is an `Exception` — there is no way to name a
//! file outside the sandbox.
//!
//! Handlers:
//!
//! - `Config { base_path }` → `ConfigResult { ok, base_path }` — the sandbox root,
//!   created if missing. Every other handler is an `Exception` until this
//!   has run.
//! - `ReadFile { path }` → `FileContent { path, content }` — UTF-8 text. A
//!   missing file, or bytes that aren't UTF-8, is an `Exception`.
//! - `WriteFile { path, content }` → `WriteResult { path, bytes }` — atomic
//!   (write to a temp file, then rename), parent directories created.
//! - `DeleteFile { path }` → `DeleteResult { path, existed }` — idempotent.
//! - `CreateDir { path }` → `CreateDirResult { path }` — recursive,
//!   idempotent.
//! - `RemoveDir { path }` → `RemoveDirResult { path, existed }` — recursive,
//!   idempotent.
//! - `ListDir { path }` → `DirListing { path, entries }` — `entries` is
//!   `[{ name, is_dir }]`, sorted by name. A missing directory is an
//!   `Exception`.
//! - `Exists { path }` → `ExistsResult { path, exists, is_file, is_dir }`.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use code_native::*;
use std::ffi::CStr;
use std::path::{Component, Path, PathBuf};
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
    guarded(&mut *out, "fs", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Config" => config(out, particle),
            "ReadFile" => read_file(out, particle),
            "WriteFile" => write_file(out, particle),
            "DeleteFile" => delete_file(out, particle),
            "CreateDir" => create_dir(out, particle),
            "RemoveDir" => remove_dir(out, particle),
            "ListDir" => list_dir(out, particle),
            "Exists" => exists(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "fs", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let raw = find_field(particle, "base_path")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or("Config requires a non-empty string 'base_path'")?;
    let base = PathBuf::from(raw);
    fs::create_dir_all(&base).map_err(|e| format!("cannot create base '{raw}': {e}"))?;
    let base = fs::canonicalize(&base).map_err(|e| format!("cannot resolve base '{raw}': {e}"))?;
    let shown = base.display().to_string();
    *BASE.lock().unwrap_or_else(|e| e.into_inner()) = Some(base);

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"ConfigResult");
    boolean(b.slot_mut(1), true);
    owned_str(b.slot_mut(2), &shown);
    object(out, &[c"_class", c"ok", c"base_path"], &mut b);
    b.release_all();
    Ok(())
}

fn read_file(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let (rel, full) = target(particle)?;
    match fs::read(&full) {
        Ok(bytes) => {
            let content = String::from_utf8(bytes)
                .map_err(|_| format!("'{rel}' is not valid UTF-8"))?;
            let mut b = SlotBuffer::new(3);
            borrowed_str(b.slot_mut(0), c"FileContent");
            owned_str(b.slot_mut(1), &rel);
            owned_str(b.slot_mut(2), &content);
            object(out, &[c"_class", c"path", c"content"], &mut b);
            b.release_all();
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Err(format!("no such file: '{rel}'")),
        Err(e) => Err(format!("cannot read '{rel}': {e}")),
    }
}

fn write_file(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let (rel, full) = target(particle)?;
    let content = find_field(particle, "content")
        .and_then(read_str)
        .ok_or("WriteFile requires a string 'content'")?;

    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("cannot create parent of '{rel}': {e}"))?;
    }
    // Atomic: a reader sees either the old file or the whole new one, never
    // a half-written one. The temp file is a sibling so `rename` stays on
    // the same filesystem.
    let tmp = full.with_file_name(format!(
        ".{}.tmp",
        full.file_name().and_then(|n| n.to_str()).unwrap_or("write")
    ));
    fs::write(&tmp, content).map_err(|e| format!("cannot write '{rel}': {e}"))?;
    if let Err(e) = fs::rename(&tmp, &full) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("cannot finalize '{rel}': {e}"));
    }

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"WriteResult");
    owned_str(b.slot_mut(1), &rel);
    number(b.slot_mut(2), content.len() as f64);
    object(out, &[c"_class", c"path", c"bytes"], &mut b);
    b.release_all();
    Ok(())
}

fn delete_file(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let (rel, full) = target(particle)?;
    let existed = match fs::remove_file(&full) {
        Ok(()) => true,
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(format!("cannot delete '{rel}': {e}")),
    };
    path_existed(out, c"DeleteResult", &rel, existed);
    Ok(())
}

fn create_dir(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let (rel, full) = target(particle)?;
    fs::create_dir_all(&full).map_err(|e| format!("cannot create dir '{rel}': {e}"))?;
    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"CreateDirResult");
    owned_str(b.slot_mut(1), &rel);
    object(out, &[c"_class", c"path"], &mut b);
    b.release_all();
    Ok(())
}

fn remove_dir(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let (rel, full) = target(particle)?;
    let existed = if full.is_dir() {
        fs::remove_dir_all(&full).map_err(|e| format!("cannot remove dir '{rel}': {e}"))?;
        true
    } else {
        false
    };
    path_existed(out, c"RemoveDirResult", &rel, existed);
    Ok(())
}

fn list_dir(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let (rel, full) = target(particle)?;
    let read = match fs::read_dir(&full) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(format!("no such directory: '{rel}'"))
        }
        Err(e) => return Err(format!("cannot list '{rel}': {e}")),
    };
    let mut entries: Vec<(String, bool)> = Vec::new();
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        entries.push((name, is_dir));
    }
    entries.sort();

    let mut list = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(entries.len());
    for (i, (name, is_dir)) in entries.iter().enumerate() {
        let mut e = SlotBuffer::new(3);
        borrowed_str(e.slot_mut(0), c"DirEntry");
        owned_str(e.slot_mut(1), name);
        boolean(e.slot_mut(2), *is_dir);
        object(buf.slot_mut(i as i64), &[c"_class", c"name", c"is_dir"], &mut e);
        e.release_all();
    }
    array(&mut list, &mut buf);
    buf.release_all();

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"DirListing");
    owned_str(b.slot_mut(1), &rel);
    copy(b.slot_mut(2), &list);
    object(out, &[c"_class", c"path", c"entries"], &mut b);
    b.release_all();
    release(&mut list);
    Ok(())
}

fn exists(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let (rel, full) = target(particle)?;
    let mut b = SlotBuffer::new(5);
    borrowed_str(b.slot_mut(0), c"ExistsResult");
    owned_str(b.slot_mut(1), &rel);
    boolean(b.slot_mut(2), full.exists());
    boolean(b.slot_mut(3), full.is_file());
    boolean(b.slot_mut(4), full.is_dir());
    object(
        out,
        &[c"_class", c"path", c"exists", c"is_file", c"is_dir"],
        &mut b,
    );
    b.release_all();
    Ok(())
}

// ---------------------------------------------------------------------------
// Path handling
// ---------------------------------------------------------------------------

/// The `path` field, and where it resolves to inside the sandbox. `Err` if
/// `Config` has not run, `path` is missing, or the path would escape the base.
fn target(particle: &CodeValue) -> Result<(String, PathBuf), String> {
    let raw = find_field(particle, "path")
        .and_then(read_str)
        .ok_or("this handler requires a string 'path'")?;
    let guard = BASE.lock().unwrap_or_else(|e| e.into_inner());
    let base = guard
        .as_ref()
        .ok_or("fs has no base — send Config { base_path } first")?;
    Ok((raw.to_string(), contain(base, raw)?))
}

/// Resolve `raw` **inside** `base`. A leading `/` is dropped (paths are
/// always base-relative), `.` is skipped, and a `..` that would leave the
/// base is refused. The result is guaranteed to be under `base` even though
/// the file it names need not exist yet.
fn contain(base: &Path, raw: &str) -> Result<PathBuf, String> {
    let mut result = base.to_path_buf();
    for component in Path::new(raw).components() {
        match component {
            Component::Normal(part) => result.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() || !result.starts_with(base) {
                    return Err(format!("path '{raw}' escapes the sandbox"));
                }
            }
            // A leading `/` or a Windows prefix: paths here are base-relative,
            // so both are simply ignored rather than treated as a new root.
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    if result.starts_with(base) {
        Ok(result)
    } else {
        Err(format!("path '{raw}' escapes the sandbox"))
    }
}

/// `{ _class, path, existed }` — the shape `Delete`/`RemoveDir` answer with.
fn path_existed(out: &mut CodeValue, class: &'static CStr, rel: &str, existed: bool) {
    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), class);
    owned_str(b.slot_mut(1), rel);
    boolean(b.slot_mut(2), existed);
    object(out, &[c"_class", c"path", c"existed"], &mut b);
    b.release_all();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contain_keeps_relative_paths_inside() {
        let base = Path::new("/srv/data");
        assert_eq!(
            contain(base, "notes/today.md").unwrap(),
            Path::new("/srv/data/notes/today.md")
        );
        assert_eq!(contain(base, "./x").unwrap(), Path::new("/srv/data/x"));
        assert_eq!(contain(base, "").unwrap(), base);
    }

    #[test]
    fn contain_drops_a_leading_slash() {
        let base = Path::new("/srv/data");
        assert_eq!(
            contain(base, "/etc/passwd").unwrap(),
            Path::new("/srv/data/etc/passwd")
        );
    }

    #[test]
    fn contain_allows_dotdot_that_stays_inside() {
        let base = Path::new("/srv/data");
        assert_eq!(
            contain(base, "a/b/../c").unwrap(),
            Path::new("/srv/data/a/c")
        );
    }

    #[test]
    fn contain_refuses_dotdot_that_escapes() {
        let base = Path::new("/srv/data");
        assert!(contain(base, "../secret").is_err());
        assert!(contain(base, "a/../../secret").is_err());
        assert!(contain(base, "/../secret").is_err());
    }
}

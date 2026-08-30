//! The `git_mock` native module — a drop-in for `git` that runs no `git`
//! and reaches no remote, for the Code programming language, written in Rust
//! on [`code-native`].
//!
//! Same particles and the same result shapes as `git`. State is a small
//! in-memory model — a current branch, a HEAD that moves on `Commit`, a
//! stash flag, a commit count — enough that a program's control flow (did
//! the commit happen? is the tree clean?) behaves as it would against a real
//! repository, without a working tree or a network.
//!
//! Handlers: `Config`, `Stash`, `StashPop`, `Init`, `Clone`, `Add`,
//! `Commit`, `Push`, `SetRemote`, `Status` — see `git`'s README for the
//! field contracts.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use code_native::*;
use std::sync::Mutex;

#[derive(Default)]
struct Repo {
    path: String,
    branch: String,
    commits: u32,
    staged: u32,
    stashed: bool,
}

static REPO: Mutex<Option<Repo>> = Mutex::new(None);

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
    guarded(&mut *out, "git_mock", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Config" => config(out, particle),
            "Stash" => stash(out, false),
            "StashPop" => stash(out, true),
            "Init" => init(out, particle),
            "Clone" => clone(out, particle),
            "Add" => add(out, particle),
            "Commit" => commit(out, particle),
            "Push" => push(out, particle),
            "SetRemote" => set_remote(out, particle),
            "Status" => status(out),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "git_mock", &message);
        }
    })
}

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let path = require_str(particle, "repo_path", "Config")?.to_string();
    let branch = opt(particle, "branch").unwrap_or_else(|| "main".to_string());
    *REPO.lock().unwrap_or_else(|e| e.into_inner()) = Some(Repo {
        path,
        branch: branch.clone(),
        ..Default::default()
    });

    let mut b = SlotBuffer::new(6);
    borrowed_str(b.slot_mut(0), c"ConfigResult");
    boolean(b.slot_mut(1), true);
    boolean(b.slot_mut(2), false); // dirty — a fresh mock repo is clean
    boolean(b.slot_mut(3), false); // stashed
    owned_str(b.slot_mut(4), &branch);
    owned_str(b.slot_mut(5), ""); // head — no commit yet
    object(
        out,
        &[c"_class", c"ok", c"dirty", c"stashed", c"branch", c"head"],
        &mut b,
    );
    b.release_all();
    Ok(())
}

fn stash(out: &mut CodeValue, pop: bool) -> Result<(), String> {
    let mut guard = REPO.lock().unwrap_or_else(|e| e.into_inner());
    let repo = guard.as_mut().ok_or(NOT_CONFIGURED)?;
    let changed = if pop {
        if !repo.stashed {
            return Err("no stash to pop".to_string());
        }
        repo.stashed = false;
        true
    } else {
        let had_work = repo.staged > 0;
        if had_work {
            repo.stashed = true;
            repo.staged = 0;
        }
        had_work
    };
    one_bool(out, c"StashResult", c"changed", changed);
    Ok(())
}

fn init(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let path = opt(particle, "path")
        .or_else(|| configured_path().ok())
        .ok_or(NOT_CONFIGURED)?;
    one_str(out, c"InitResult", c"path", &path);
    Ok(())
}

fn clone(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    require_str(particle, "url", "Clone")?;
    let path = opt(particle, "path")
        .or_else(|| configured_path().ok())
        .ok_or(NOT_CONFIGURED)?;
    one_str(out, c"CloneResult", c"path", &path);
    Ok(())
}

fn add(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let pattern = opt(particle, "pattern").unwrap_or_else(|| ".".to_string());
    let mut guard = REPO.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_mut().ok_or(NOT_CONFIGURED)?.staged += 1;
    one_str(out, c"AddResult", c"pattern", &pattern);
    Ok(())
}

fn commit(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let message = require_str(particle, "message", "Commit")?.to_string();
    let allow_empty = read_field_bool(particle, "allow_empty").unwrap_or(false);
    let mut guard = REPO.lock().unwrap_or_else(|e| e.into_inner());
    let repo = guard.as_mut().ok_or(NOT_CONFIGURED)?;
    if repo.staged == 0 && !allow_empty {
        return Err("nothing to commit — stage changes with Add, or pass allow_empty".to_string());
    }
    repo.staged = 0;
    repo.commits += 1;
    let head = fake_hash(&repo.branch, repo.commits);

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"CommitResult");
    owned_str(b.slot_mut(1), &message);
    owned_str(
        b.slot_mut(2),
        &format!("[{} {head}] {message}", repo.branch),
    );
    object(out, &[c"_class", c"message", c"output"], &mut b);
    b.release_all();
    Ok(())
}

fn push(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let remote = opt(particle, "remote").unwrap_or_else(|| "origin".to_string());
    let branch = opt(particle, "branch").unwrap_or_else(|| "HEAD".to_string());
    let _ = REPO
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .ok_or(NOT_CONFIGURED)?;

    let mut b = SlotBuffer::new(4);
    borrowed_str(b.slot_mut(0), c"PushResult");
    owned_str(b.slot_mut(1), &remote);
    owned_str(b.slot_mut(2), &branch);
    owned_str(b.slot_mut(3), "");
    object(out, &[c"_class", c"remote", c"branch", c"output"], &mut b);
    b.release_all();
    Ok(())
}

fn set_remote(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let name = require_str(particle, "name", "SetRemote")?.to_string();
    let url = require_str(particle, "url", "SetRemote")?.to_string();
    let _ = REPO
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .ok_or(NOT_CONFIGURED)?;

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"SetRemoteResult");
    owned_str(b.slot_mut(1), &name);
    owned_str(b.slot_mut(2), &mask(&url));
    object(out, &[c"_class", c"name", c"url"], &mut b);
    b.release_all();
    Ok(())
}

fn status(out: &mut CodeValue) -> Result<(), String> {
    let guard = REPO.lock().unwrap_or_else(|e| e.into_inner());
    let repo = guard.as_ref().ok_or(NOT_CONFIGURED)?;
    let output = if repo.staged > 0 {
        "A  (mock staged change)\n".to_string()
    } else {
        String::new()
    };
    let clean = output.is_empty();

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"StatusResult");
    owned_str(b.slot_mut(1), &output);
    boolean(b.slot_mut(2), clean);
    object(out, &[c"_class", c"output", c"clean"], &mut b);
    b.release_all();
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

const NOT_CONFIGURED: &str = "git_mock has no repository — send Config { repo_path } first";

fn configured_path() -> Result<String, String> {
    REPO.lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|r| r.path.clone())
        .ok_or(NOT_CONFIGURED.to_string())
}

/// A short hex-ish string that is stable for a given branch and commit
/// count — a HEAD a test can assert moved without pinning the exact value.
fn fake_hash(branch: &str, n: u32) -> String {
    let mut h: u64 = 1469598103934665603;
    for byte in branch.bytes().chain(n.to_le_bytes()) {
        h ^= byte as u64;
        h = h.wrapping_mul(1099511628211);
    }
    format!("{:07x}", h & 0xfff_ffff)
}

/// Mask `user:pass@` in a URL, as the real `git` module does before it puts
/// a remote URL in a result.
fn mask(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => match rest.split_once('@') {
            Some((_creds, host)) => format!("{scheme}://***@{host}"),
            None => url.to_string(),
        },
        None => url.to_string(),
    }
}

fn require_str<'a>(particle: &'a CodeValue, name: &str, class: &str) -> Result<&'a str, String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{class} requires a non-empty string '{name}'"))
}

fn opt(particle: &CodeValue, name: &str) -> Option<String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
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

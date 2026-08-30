//! The `git` native module — version control over the system `git` binary,
//! for the Code programming language, written in Rust on [`code-native`].
//!
//! A thin adapter: every handler shells out to `git`, and authentication is
//! whatever the host's SSH agent / credential helper already provides. `git`
//! must be on `PATH`.
//!
//! Handlers:
//!
//! - `Config { repo_path, remote_url?, branch?, on_dirty? }` →
//!   `ConfigResult { ok, dirty, stashed, branch, head }` — the setup
//!   particle. Sets the repository every other handler works in, and checks
//!   what state it's in before anything runs:
//!   - the folder is already a *different* repo (`origin` doesn't match
//!     `remote_url`, or the path is inside another repo's tree) → `Exception`
//!   - the folder isn't a repo yet → `git init` (and `origin` if `remote_url`)
//!   - `branch` given → `git checkout` it
//!   - the working tree of a pre-existing repo is dirty → `on_dirty` decides:
//!     `"error"` (default, an `Exception`), `"stash"` (`git stash`, reported
//!     as `stashed = true`), or `"ignore"` (proceed, `dirty = true`)
//! - `Stash {}` / `StashPop {}` → `StashResult { changed }` — manual stash
//!   control, for an app that handled `dirty` itself.
//! - `Init { path? }` → `InitResult { path }` — `git init`, idempotent.
//! - `Clone { url, path? }` → `CloneResult { path }` — `git clone`.
//! - `Add { pattern? }` → `AddResult { pattern }` — `git add` (default `.`).
//! - `Commit { message, author_name?, author_email?, allow_empty? }` →
//!   `CommitResult { message, output }`.
//! - `Push { remote?, branch? }` → `PushResult { remote, branch, output }`.
//! - `SetRemote { name, url }` → `SetRemoteResult { name, url }`.
//! - `Status {}` → `StatusResult { output, clean }` — `git status --porcelain`.
//!
//! A `git` command that exits non-zero is an `Exception` carrying its
//! stderr, with any `user:pass@` in a URL masked.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use code_native::*;
use std::ffi::CStr;
use std::process::Command;
use std::sync::Mutex;

static REPO: Mutex<Option<String>> = Mutex::new(None);

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
    guarded(&mut *out, "git", |out| {
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
            exception(out, "git", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let repo = require_str(particle, "repo_path", "Config")?.to_string();
    std::fs::create_dir_all(&repo).map_err(|e| format!("cannot create '{repo}': {e}"))?;
    let canon = std::fs::canonicalize(&repo)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| repo.clone());
    let remote_url = find_field(particle, "remote_url")
        .and_then(read_str)
        .filter(|s| !s.is_empty());

    // Is there already a repository here, and is it *this* one?
    let existing = git(&repo, &["rev-parse", "--show-toplevel"]).ok();
    match &existing {
        Some(top) => {
            let top_canon = std::fs::canonicalize(top)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| top.clone());
            if top_canon != canon {
                return Err(format!(
                    "'{repo}' is inside the git repository at '{top}', not a repository of its own"
                ));
            }
            if let Some(url) = remote_url {
                if let Ok(origin) = git(&repo, &["remote", "get-url", "origin"]) {
                    if !origin.is_empty() && origin != url {
                        return Err(format!(
                            "'{repo}' already tracks {}, not {}",
                            mask(&origin),
                            mask(url)
                        ));
                    }
                }
                if git(&repo, &["remote", "add", "origin", url]).is_err() {
                    git(&repo, &["remote", "set-url", "origin", url])?;
                }
            }
        }
        None => {
            git(&repo, &["init", "--quiet"])?;
            if let Some(url) = remote_url {
                let _ = git(&repo, &["remote", "add", "origin", url]);
            }
        }
    }

    if let Some(branch) = find_field(particle, "branch")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
    {
        git(&repo, &["checkout", branch]).map_err(|e| format!("cannot checkout '{branch}': {e}"))?;
    }

    // Only a *pre-existing* repo's dirt is worth protecting: a fresh `init`
    // has no history to lose, so its untracked files just come along.
    let mut dirty = !git(&repo, &["status", "--porcelain"])?.is_empty();
    let mut stashed = false;
    if dirty && existing.is_some() {
        match find_field(particle, "on_dirty").and_then(read_str).unwrap_or("error") {
            "error" => {
                return Err(format!(
                    "the working tree at '{repo}' has uncommitted changes — \
                     set on_dirty to \"stash\" or \"ignore\""
                ))
            }
            "stash" => {
                git(&repo, &["stash", "push", "--include-untracked"])?;
                stashed = true;
                dirty = false;
            }
            "ignore" => {}
            other => {
                return Err(format!(
                    "on_dirty must be \"error\", \"stash\" or \"ignore\", not \"{other}\""
                ))
            }
        }
    }

    let branch = git(&repo, &["branch", "--show-current"]).unwrap_or_default();
    let head = git(&repo, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();

    *REPO.lock().unwrap_or_else(|e| e.into_inner()) = Some(repo);

    let mut b = SlotBuffer::new(6);
    borrowed_str(b.slot_mut(0), c"ConfigResult");
    boolean(b.slot_mut(1), true);
    boolean(b.slot_mut(2), dirty);
    boolean(b.slot_mut(3), stashed);
    owned_str(b.slot_mut(4), &branch);
    owned_str(b.slot_mut(5), &head);
    object(
        out,
        &[c"_class", c"ok", c"dirty", c"stashed", c"branch", c"head"],
        &mut b,
    );
    b.release_all();
    Ok(())
}

/// `Stash {}` (`pop = false`) / `StashPop {}` (`pop = true`) →
/// `StashResult { changed }`. `changed = false` from `Stash` means there was
/// nothing to stash; `StashPop` with no stash is an `Exception`.
fn stash(out: &mut CodeValue, pop: bool) -> Result<(), String> {
    let repo = configured()?;
    let changed = if pop {
        git(&repo, &["stash", "pop"])?;
        true
    } else {
        let before = git(&repo, &["stash", "list"])?;
        git(&repo, &["stash", "push", "--include-untracked"])?;
        git(&repo, &["stash", "list"])? != before
    };
    one_bool(out, c"StashResult", c"changed", changed);
    Ok(())
}

fn init(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let path = match find_field(particle, "path").and_then(read_str) {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => configured()?,
    };
    std::fs::create_dir_all(&path).map_err(|e| format!("cannot create '{path}': {e}"))?;
    git(&path, &["init", "--quiet"])?;
    one_str(out, c"InitResult", c"path", &path);
    Ok(())
}

fn clone(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let url = require_str(particle, "url", "Clone")?;
    let path = match find_field(particle, "path").and_then(read_str) {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => configured()?,
    };
    // `clone` takes no `-C`, so run it plainly.
    run(Command::new("git").args(["clone", url, &path]))?;
    one_str(out, c"CloneResult", c"path", &path);
    Ok(())
}

fn add(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let pattern = find_field(particle, "pattern")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(".");
    git(&configured()?, &["add", pattern])?;
    one_str(out, c"AddResult", c"pattern", pattern);
    Ok(())
}

fn commit(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let message = require_str(particle, "message", "Commit")?;
    let name = find_field(particle, "author_name")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("code");
    let email = find_field(particle, "author_email")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("code@localhost");
    let allow_empty = read_field_bool(particle, "allow_empty").unwrap_or(false);

    // `-c` here rather than a repo-config write: a module that mutated the
    // repo's config would be a surprise, and CI repos have no user set.
    let name_cfg = format!("user.name={name}");
    let email_cfg = format!("user.email={email}");
    let mut args = vec!["-c", &name_cfg, "-c", &email_cfg, "commit", "-m", message, "-q"];
    if allow_empty {
        args.push("--allow-empty");
    }
    let output = git(&configured()?, &args)?;

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"CommitResult");
    owned_str(b.slot_mut(1), message);
    owned_str(b.slot_mut(2), &output);
    object(out, &[c"_class", c"message", c"output"], &mut b);
    b.release_all();
    Ok(())
}

fn push(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let remote = find_field(particle, "remote")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("origin");
    let branch = find_field(particle, "branch")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("HEAD");
    let output = git(&configured()?, &["push", remote, branch])?;

    let mut b = SlotBuffer::new(4);
    borrowed_str(b.slot_mut(0), c"PushResult");
    owned_str(b.slot_mut(1), remote);
    owned_str(b.slot_mut(2), branch);
    owned_str(b.slot_mut(3), &output);
    object(out, &[c"_class", c"remote", c"branch", c"output"], &mut b);
    b.release_all();
    Ok(())
}

fn set_remote(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let name = require_str(particle, "name", "SetRemote")?;
    let url = require_str(particle, "url", "SetRemote")?;
    let repo = configured()?;
    if git(&repo, &["remote", "add", name, url]).is_err() {
        git(&repo, &["remote", "set-url", name, url])?;
    }
    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"SetRemoteResult");
    owned_str(b.slot_mut(1), name);
    owned_str(b.slot_mut(2), &mask(url));
    object(out, &[c"_class", c"name", c"url"], &mut b);
    b.release_all();
    Ok(())
}

fn status(out: &mut CodeValue) -> Result<(), String> {
    let output = git(&configured()?, &["status", "--porcelain"])?;
    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"StatusResult");
    owned_str(b.slot_mut(1), &output);
    boolean(b.slot_mut(2), output.is_empty());
    object(out, &[c"_class", c"output", c"clean"], &mut b);
    b.release_all();
    Ok(())
}

// ---------------------------------------------------------------------------
// Running git
// ---------------------------------------------------------------------------

/// `git -C <repo> <args…>`, trimmed stdout on success, stderr as an `Err`.
fn git(repo: &str, args: &[&str]) -> Result<String, String> {
    run(Command::new("git").arg("-C").arg(repo).args(args))
}

fn run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd
        .output()
        .map_err(|e| format!("cannot run git — is it installed? ({e})"))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let detail = if stderr.trim().is_empty() {
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    } else {
        stderr.trim().to_string()
    };
    Err(mask(&detail))
}

/// Replace `scheme://user:pass@host` with `scheme://****@host` so a
/// credential in a URL never lands in an `Exception` message or a result.
fn mask(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(sep) = rest.find("://") {
        let after_scheme = sep + 3;
        // The authority ends at the first '/', '?', or whitespace.
        let authority_end = rest[after_scheme..]
            .find(|c: char| c == '/' || c == '?' || c.is_whitespace())
            .map(|i| after_scheme + i)
            .unwrap_or(rest.len());
        let authority = &rest[after_scheme..authority_end];
        out.push_str(&rest[..after_scheme]);
        match authority.rfind('@') {
            Some(at) => {
                out.push_str("****");
                out.push_str(&authority[at..]);
            }
            None => out.push_str(authority),
        }
        rest = &rest[authority_end..];
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

fn configured() -> Result<String, String> {
    REPO.lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or_else(|| "git has no repository — send Config { repo_path } first".to_string())
}

fn require_str<'a>(particle: &'a CodeValue, name: &str, class: &str) -> Result<&'a str, String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{class} requires a non-empty string '{name}'"))
}

fn one_str(out: &mut CodeValue, class: &'static CStr, key: &'static CStr, value: &str) {
    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), class);
    owned_str(b.slot_mut(1), value);
    object(out, &[c"_class", key], &mut b);
    b.release_all();
}

fn one_bool(out: &mut CodeValue, class: &'static CStr, key: &'static CStr, value: bool) {
    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), class);
    boolean(b.slot_mut(1), value);
    object(out, &[c"_class", key], &mut b);
    b.release_all();
}

#[cfg(test)]
mod tests {
    use super::mask;

    #[test]
    fn mask_hides_a_url_credential() {
        assert_eq!(
            mask("fatal: cannot access https://user:tok3n@github.com/o/r.git/"),
            "fatal: cannot access https://****@github.com/o/r.git/"
        );
    }

    #[test]
    fn mask_leaves_a_plain_url_alone() {
        assert_eq!(
            mask("cloning https://github.com/o/r.git failed"),
            "cloning https://github.com/o/r.git failed"
        );
    }

    #[test]
    fn mask_handles_ssh_and_no_scheme() {
        // `git@github.com:o/r` has no `://`, so nothing to do.
        assert_eq!(mask("git@github.com:o/r.git"), "git@github.com:o/r.git");
    }
}

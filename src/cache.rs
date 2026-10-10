//! The shared build cache (tickets 111 and 118): `CODE_CACHE_DIR`, else
//! `$XDG_CACHE_HOME/code`, else `~/.cache/code` — one place for every
//! project, as Go's GOCACHE and Zig's global cache are.
//!
//! What it holds is keyed by content only — what went in, never a name or a
//! date — so an entry can be reused but never be the wrong one. Entries are
//! written beside and renamed into place, so two builds at once never read
//! half a file; a hit refreshes the entry's time, and when the cache passes
//! its size limit (`CODE_CACHE_LIMIT` bytes, default 1 GB) the entries used
//! least recently are removed. `CODE_CACHE=0` turns it off; `code cache`
//! says where it is and how big, `code cache clean` empties it.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const DEFAULT_LIMIT: u64 = 1 << 30;

/// The cache's directory, or `None` when it is turned off or there is no
/// home to put it in.
pub fn dir() -> Option<PathBuf> {
    if std::env::var("CODE_CACHE").is_ok_and(|v| v == "0") {
        return None;
    }
    let from = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    from("CODE_CACHE_DIR")
        .or_else(|| from("XDG_CACHE_HOME").map(|d| d.join("code")))
        .or_else(|| from("HOME").map(|d| d.join(".cache").join("code")))
}

/// A 128-bit content key, as hex: two independent 64-bit hashes (FNV-1a and
/// std's SipHash) of every part, each part length-prefixed so that two
/// different splits of the same bytes cannot meet.
pub fn key(parts: &[&[u8]]) -> String {
    use std::hash::Hasher;
    let mut fnv: u64 = 0xcbf2_9ce4_8422_2325;
    let mut sip = std::collections::hash_map::DefaultHasher::new();
    for part in parts {
        let len = (part.len() as u64).to_le_bytes();
        for byte in len.iter().chain(part.iter()) {
            fnv ^= u64::from(*byte);
            fnv = fnv.wrapping_mul(0x0100_0000_01b3);
        }
        sip.write(&len);
        sip.write(part);
    }
    format!("{fnv:016x}{:016x}", sip.finish())
}

/// The entry `name` in the cache's `kind` directory, when it is there; its
/// time is refreshed, since it was just used.
pub fn get(kind: &str, name: &str) -> Option<PathBuf> {
    let path = dir()?.join(kind).join(name);
    if !path.is_file() {
        return None;
    }
    if let Ok(file) = fs::File::options().write(true).open(&path) {
        let _ = file.set_modified(SystemTime::now());
    }
    Some(path)
}

/// Keeps a copy of `from` as `kind/name`. Best effort: a cache that cannot
/// be written only means the next build does the work again.
pub fn put(kind: &str, name: &str, from: &Path) {
    let Some(dir) = dir().map(|d| d.join(kind)) else {
        return;
    };
    let partial = dir.join(format!(
        "{name}.{}.{:?}.partial",
        std::process::id(),
        std::thread::current().id()
    ));
    let kept = fs::create_dir_all(&dir).is_ok()
        && fs::copy(from, &partial).is_ok()
        && fs::rename(&partial, dir.join(name)).is_ok();
    if !kept {
        let _ = fs::remove_file(&partial);
    }
}

/// Every entry in the cache, with its size and last use.
fn entries(root: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let mut found = Vec::new();
    let Ok(kinds) = fs::read_dir(root) else {
        return found;
    };
    for kind in kinds.flatten() {
        let Ok(files) = fs::read_dir(kind.path()) else {
            continue;
        };
        for file in files.flatten() {
            if let Ok(meta) = file.metadata() {
                if meta.is_file() {
                    let used = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                    found.push((file.path(), meta.len(), used));
                }
            }
        }
    }
    found
}

/// The cache's size in bytes and its number of entries.
pub fn size() -> (u64, usize) {
    let Some(root) = dir() else {
        return (0, 0);
    };
    let all = entries(&root);
    (all.iter().map(|(_, len, _)| len).sum(), all.len())
}

/// Removes the least recently used entries while the cache is over its
/// limit, down to nine tenths of it.
pub fn trim() {
    let Some(root) = dir() else {
        return;
    };
    let limit = std::env::var("CODE_CACHE_LIMIT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_LIMIT);
    let mut all = entries(&root);
    let mut total: u64 = all.iter().map(|(_, len, _)| len).sum();
    if total <= limit {
        return;
    }
    all.sort_by_key(|(_, _, used)| *used);
    for (path, len, _) in all {
        if total <= limit / 10 * 9 {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

/// Empties the cache.
pub fn clean() -> std::io::Result<()> {
    match dir() {
        Some(root) if root.exists() => fs::remove_dir_all(root),
        _ => Ok(()),
    }
}

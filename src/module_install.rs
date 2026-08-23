//! Module installation: the manifest, the lockfile, the index, and the
//! download-and-verify flow behind `code install` / `remove` / `ls`.
//!
//! Everything here except [`fetch_url`] and [`download_to`] is plain
//! filesystem work over JSON, which keeps it unit-testable without a network
//! or a release to fetch. The layout managed is the one agreed in
//! `docs/todo/community-modules.md` ("Install: `code install`"):
//!
//! ```text
//! <project>/.code/modules/<name>/<version>/   # installed bytes
//! <project>/.code/lock.json                   # what pins them
//! ~/.code/modules/<name>/<version>/           # global installs
//! ```
//!
//! Verification is two-sided: the sha256 is checked right after download,
//! and re-checked at load time while a lock entry exists (see `loader.rs`'s
//! fallback chain) — a tampered or replaced `.so` fails loudly instead of
//! loading. Artifact signing is a later phase, not day one.
//!
//! Gated behind the `install` feature: the wasm host has no filesystem story
//! at all (`NoModules`) and should not pay for serde in the bundle.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const LOCK_FILE_NAME: &str = "lock.json";

/// The directory installed modules live in under a `.code` directory —
/// defined in `loader.rs` because the resolver's fallback chain needs it
/// even when the installer isn't compiled in.
const MODULES_DIR_NAME: &str = crate::loader::MODULES_DIR;

/// One platform's asset inside a module manifest.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct PlatformAsset {
    /// Asset filename within the release, e.g. `terminal-linux-x86_64.so`.
    pub asset: String,
    /// Lowercase hex sha256 of that asset.
    pub sha256: String,
}

/// A published module's `module.json` — what CI writes next to each release's
/// assets (see `.github/workflows/publish-modules.yml`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Manifest {
    pub name: String,
    /// SemVer without a leading `v` — the tag carries the `v`, the manifest
    /// does not.
    pub version: String,
    pub abi_version: u32,
    #[serde(default)]
    pub handlers: Vec<String>,
    #[serde(default)]
    pub vars: Vec<String>,
    /// Keyed by platform triple, e.g. `linux-x86_64`.
    pub platforms: BTreeMap<String, PlatformAsset>,
}

impl Manifest {
    fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("malformed module manifest: {e}"))
    }

    /// The asset for `platform`, if the manifest covers it.
    pub fn platform(&self, platform: &str) -> Option<&PlatformAsset> {
        self.platforms.get(platform)
    }
}

/// One row of the project index — the JSON file served from the Pages site
/// that maps a first-party module name to its latest version and release.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct IndexEntry {
    /// Latest published semVer (no leading `v`).
    pub version: String,
    /// The GitHub Release page the assets live under.
    pub url: String,
}

/// The project index: first-party module name → latest release.
pub type Index = BTreeMap<String, IndexEntry>;

/// One pinned module in `.code/lock.json`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LockEntry {
    pub name: String,
    pub version: String,
    /// Where the bytes came from — the release URL for index installs, the
    /// full URL given verbatim for community installs. Provenance, not
    /// authority: verification is the hash.
    pub source: String,
    /// The asset filename the bytes were downloaded as.
    pub asset: String,
    /// Lowercase hex sha256 of the installed bytes. Re-checked at load time.
    pub sha256: String,
    /// `true` when the module lives in `~/.code/modules/` rather than the
    /// project-local directory.
    #[serde(default)]
    pub global: bool,
}

/// The project's `.code/lock.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Lockfile {
    #[serde(default)]
    pub modules: BTreeMap<String, LockEntry>,
}

impl Lockfile {
    fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("malformed lockfile: {e}"))
    }

    fn render(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default() + "\n"
    }
}

/// Where a module's bytes live for a given script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallScope {
    /// `<nearest .code dir>/modules/...` — the default.
    Project,
    /// `~/.code/modules/...` — `code install --global`.
    Global,
}

/// The host triple our artifacts are keyed by, e.g. `linux-x86_64`.
///
/// Read from the process environment once per call — cheap, and the only
/// place in this module that touches the host at all. Tests pin expectations
/// against whatever machine runs them, which is exactly the contract: the
/// manifest must cover *this* machine.
pub fn current_platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-arm64",
        ("macos", "x86_64") => "macos-x86_64",
        ("macos", "aarch64") => "macos-arm64",
        ("windows", "x86_64") => "windows-x86_64",
        other => panic!("unsupported host for module installs: {other:?}"),
    }
}

/// The project `.code` directory for `cwd`, creating `<cwd>/.code` when no
/// ancestor has one — installing somewhere must always have somewhere to go,
/// exactly as `npm install` creates `node_modules/` on demand.
pub fn ensure_project_code_dir(cwd: &Path) -> Result<PathBuf, String> {
    // The walk-up lives in `loader.rs` (always compiled): one definition of
    // "what counts as this project" for both the resolver and the installer.
    if let Some(dir) = crate::loader::find_project_code_dir(cwd) {
        return Ok(dir);
    }
    let dir = cwd.join(".code");
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create '{}': {e}", dir.display()))?;
    Ok(dir)
}

/// The user-global `.code` directory (`~/.code`).
pub fn global_code_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| h.join(".code"))
}

/// The modules root for `scope` relative to `cwd`: either the nearest
/// project `.code/modules` or `~/.code/modules`.
pub fn modules_root(cwd: &Path, scope: InstallScope) -> Option<PathBuf> {
    match scope {
        InstallScope::Project => {
            crate::loader::find_project_code_dir(cwd).map(|c| c.join(MODULES_DIR_NAME))
        }
        InstallScope::Global => global_code_dir().map(|g| g.join(MODULES_DIR_NAME)),
    }
}

/// The lockfile path for `cwd`: the nearest project `.code/lock.json`. There
/// is deliberately no global lockfile — a global install is recorded in the
/// lockfile of whichever project installs it, flagged `"global": true`.
pub fn lockfile_path(cwd: &Path) -> Option<PathBuf> {
    crate::loader::find_project_code_dir(cwd).map(|c| c.join(LOCK_FILE_NAME))
}

/// Load the lockfile at `path`, treating a missing file as an empty one.
pub fn read_lockfile(path: &Path) -> Result<Lockfile, String> {
    match fs::read_to_string(path) {
        Ok(text) => Lockfile::parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Lockfile::default()),
        Err(e) => Err(format!("cannot read '{}': {e}", path.display())),
    }
}

/// Write the lockfile via a sibling temp file plus rename, so a crash
/// mid-write cannot leave a truncated one behind.
pub fn write_lockfile(path: &Path, lock: &Lockfile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create '{}': {e}", parent.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, lock.render())
        .and_then(|_| fs::rename(&tmp, path))
        .map_err(|e| format!("cannot write '{}': {e}", path.display()))
}

/// The installed location of `name@version` under `root`, creating the
/// directories on the way down.
pub fn install_dir(root: &Path, name: &str, version: &str) -> Result<PathBuf, String> {
    let dir = root.join(name).join(version);
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create '{}': {e}", dir.display()))?;
    Ok(dir)
}

/// Fetch `url` with curl. Returns stdout on success.
///
/// curl rather than a Rust HTTP stack: the binary already shells out to the
/// system toolchain (`cc`, `nm`), and a static TLS dependency would bloat
/// every build of `code` for one subcommand. The failure message carries
/// curl's stderr, which is usually enough to act on.
pub fn fetch_url(url: &str) -> Result<String, String> {
    let output = std::process::Command::new("curl")
        .args(["-sfL", "--max-time", "60", url])
        .output()
        .map_err(|e| format!("failed to run curl: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "failed to fetch '{url}': {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Download `url` straight to `dest` (streaming, no full-body allocation).
pub fn download_to(url: &str, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create '{}': {e}", parent.display()))?;
    }
    let status = std::process::Command::new("curl")
        .args(["-fSL", "--max-time", "300", "-o"])
        .arg(dest)
        .arg(url)
        .status()
        .map_err(|e| format!("failed to run curl: {e}"))?;
    if !status.success() {
        let _ = fs::remove_file(dest);
        return Err(format!(
            "failed to download '{url}' to '{}'",
            dest.display()
        ));
    }
    Ok(())
}

/// Lowercase hex sha256 of a file, via `sha256sum` (coreutils — same
/// toolchain assumption as `cc`/`nm`).
pub fn sha256_of(path: &Path) -> Result<String, String> {
    let output = std::process::Command::new("sha256sum")
        .arg(path)
        .output()
        .map_err(|e| format!("failed to run sha256sum: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "sha256sum failed on '{}': {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace()
        .next()
        .map(str::to_lowercase)
        .ok_or_else(|| format!("unexpected sha256sum output for '{}'", path.display()))
}

/// Verify `path`'s contents against `expected` (hex, case-insensitive).
pub fn verify_sha256(path: &Path, expected: &str) -> Result<bool, String> {
    let actual = sha256_of(path)?;
    Ok(actual.eq_ignore_ascii_case(expected.trim()))
}

/// Split a GitHub Releases tag URL into the base its downloadable assets
/// live under: `…/releases/tag/v1.0.0` → `…/releases/download/v1.0.0`.
pub fn release_asset_base(release_url: &str) -> Result<String, String> {
    let trimmed = release_url.trim_end_matches('/');
    let idx = trimmed.find("/releases/tag/").ok_or_else(|| {
        format!("'{release_url}' is not a GitHub Releases tag URL (expected …/releases/tag/TAG)")
    })?;
    Ok(format!(
        "{}/releases/download/{}",
        &trimmed[..idx],
        &trimmed[idx + "/releases/tag/".len()..]
    ))
}

/// The manifest URL for a source URL: a GitHub Releases *tag* page serves
/// HTML, so the manifest lives at `{page}/{name}.json` beside the assets;
/// any other URL is taken to be the manifest itself (community installs pass
/// the manifest URL directly).
pub fn manifest_url_for(source: &str, name: &str) -> String {
    if source.contains("/releases/tag/") {
        format!("{source}/{name}.json")
    } else {
        source.to_string()
    }
}

/// Resolve an install reference to `(name, version, source_url)` by fetching
/// its manifest.
///
/// - `https://…` → a community module installed by URL: the URL must point
///   at the manifest itself (a release page would serve HTML, not JSON);
///   name and version come from the fetched manifest, the source is the URL
///   verbatim.
/// - anything else → a first-party name looked up in `index`; the source is
///   the indexed release URL.
pub fn resolve_reference(
    reference: &str,
    index: &Index,
) -> Result<(String, String, String), String> {
    if reference.starts_with("http://") || reference.starts_with("https://") {
        // Community-by-URL: the manifest names itself.
        let manifest = Manifest::parse(&fetch_url(reference)?)?;
        Ok((manifest.name, manifest.version, reference.to_string()))
    } else {
        let entry = index.get(reference).ok_or_else(|| {
            format!(
                "unknown module '{reference}' (not in the index — community modules install \
                 by the URL of their manifest)"
            )
        })?;
        let manifest = Manifest::parse(&fetch_url(&manifest_url_for(&entry.url, reference))?)?;
        if manifest.name != reference {
            return Err(format!(
                "index lists '{reference}' but its manifest says '{}' — refusing to install",
                manifest.name
            ));
        }
        Ok((manifest.name, manifest.version, entry.url.clone()))
    }
}

/// The outcome of an install, for the CLI to report.
pub struct InstalledModule {
    pub name: String,
    pub version: String,
    /// Where the bytes landed.
    pub path: PathBuf,
    pub sha256: String,
    pub global: bool,
}

/// Install `reference` (first-party name or full URL) into `scope`, from
/// `cwd`. Downloads the asset for the current platform, verifies its sha256
/// against the manifest, lays it down under `<root>/<name>/<version>/`, and
/// records a lock entry. An existing identical install is left alone; a
/// different one is replaced.
pub fn install(
    cwd: &Path,
    reference: &str,
    scope: InstallScope,
    index: &Index,
) -> Result<InstalledModule, String> {
    let (name, version, source) = resolve_reference(reference, index)?;

    // Nearest project wins; a bare directory becomes a project on demand.
    let code_dir = ensure_project_code_dir(cwd)?;
    let root = match scope {
        InstallScope::Project => code_dir.join(MODULES_DIR_NAME),
        InstallScope::Global => global_code_dir()
            .map(|g| g.join(MODULES_DIR_NAME))
            .ok_or_else(|| "cannot locate ~/.code to install into (no HOME?)".to_string())?,
    };

    // The manifest again here rather than threaded through
    // resolve_reference: it carries the per-platform asset table, and
    // re-fetching one small JSON document is cheaper than reshaping the
    // resolver's signature.
    let manifest_url = manifest_url_for(&source, &name);
    let manifest = Manifest::parse(&fetch_url(&manifest_url)?)?;
    if manifest.name != name || manifest.version != version {
        return Err(format!(
            "manifest mismatch: resolved {name}@{version} but manifest says {}@{}",
            manifest.name, manifest.version
        ));
    }

    let platform = current_platform();
    let asset = manifest
        .platform(platform)
        .ok_or_else(|| format!("module '{name}' has no artifact for this platform ({platform})"))?;

    let dir = install_dir(&root, &name, &version)?;
    let dest = dir.join(asset.asset.clone());

    // Replace any previous bytes for this name+version before downloading,
    // so a failed download never leaves stale-but-pinned files behind.
    let _ = fs::remove_file(&dest);
    let asset_url = if source.contains("/releases/tag/") {
        let base = release_asset_base(&source)?;
        format!("{base}/{}", asset.asset)
    } else {
        // Direct-URL source: the manifest was fetched from there, so the
        // assets sit alongside it — swap the trailing manifest name for the
        // asset name.
        let stem = manifest_url.trim_end_matches(&format!("/{name}.json"));
        format!("{stem}/{}", asset.asset)
    };
    download_to(&asset_url, &dest)?;

    if !verify_sha256(&dest, &asset.sha256)? {
        let _ = fs::remove_file(&dest);
        return Err(format!(
            "sha256 mismatch for '{name}@{version}' — the downloaded bytes do not match the \
             manifest; refusing to install (partial file removed)"
        ));
    }

    // Record the lock entry in the project's .code dir. A global install
    // still lands in the *project's* lockfile, flagged — there is no global
    // lockfile by design.
    let lock_path = code_dir.join(LOCK_FILE_NAME);
    let mut lock = read_lockfile(&lock_path)?;
    lock.modules.insert(
        name.clone(),
        LockEntry {
            name: name.clone(),
            version: version.clone(),
            source: source.clone(),
            asset: asset.asset.clone(),
            sha256: asset.sha256.clone(),
            global: scope == InstallScope::Global,
        },
    );
    write_lockfile(&lock_path, &lock)?;

    Ok(InstalledModule {
        name,
        version,
        path: dest,
        sha256: asset.sha256.clone(),
        global: scope == InstallScope::Global,
    })
}

/// Remove `name` from `cwd`'s lockfile and delete its installed bytes in
/// both scopes. Missing pieces are reported, not errors — removal should be
/// idempotent.
pub fn remove(cwd: &Path, name: &str) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();

    let mut removed_entry = false;
    if let Some(lock_path) = lockfile_path(cwd) {
        let mut lock = read_lockfile(&lock_path)?;
        if let Some(entry) = lock.modules.remove(name) {
            removed_entry = true;
            for root in [
                modules_root(cwd, InstallScope::Project),
                modules_root(cwd, InstallScope::Global),
            ]
            .into_iter()
            .flatten()
            {
                let dir = root.join(&entry.name).join(&entry.version);
                match fs::remove_dir_all(&dir) {
                    Ok(()) => notes.push(format!("removed {}", dir.display())),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => notes.push(format!("left '{}' ({e})", dir.display())),
                }
            }
            write_lockfile(&lock_path, &lock)?;
        }
    }

    if !removed_entry {
        notes.push(format!(
            "'{name}' is not in the lockfile — nothing to remove"
        ));
    }
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> &'static str {
        r#"{
          "name": "terminal",
          "version": "1.0.0",
          "abi_version": 1,
          "handlers": ["echo"],
          "vars": [],
          "platforms": {
            "linux-x86_64": {
              "asset": "terminal-linux-x86_64.so",
              "sha256": "abc123def456abc123def456abc123def456abc123def456abc123def456abc123"
            }
          }
        }"#
    }

    #[test]
    fn manifest_parses_and_looks_up_platforms() {
        let m = Manifest::parse(sample_manifest()).expect("sample manifest parses");
        assert_eq!(m.name, "terminal");
        assert_eq!(m.version, "1.0.0");
        assert_eq!(m.abi_version, 1);
        assert_eq!(m.handlers, vec!["echo"]);
        let asset = m.platform(current_platform());
        if current_platform() == "linux-x86_64" {
            assert!(
                asset.is_some(),
                "this machine must be covered by the sample"
            );
            assert_eq!(asset.unwrap().asset, "terminal-linux-x86_64.so");
        } else {
            assert!(asset.is_none());
        }
        assert!(m.platform("no-such-platform").is_none());
    }

    #[test]
    fn manifest_rejects_garbage() {
        assert!(Manifest::parse("not json").is_err());
        assert!(
            Manifest::parse(r#"{"name":"x"}"#).is_err(),
            "missing fields must fail"
        );
    }

    #[test]
    fn lockfile_round_trips() {
        let mut lock = Lockfile::default();
        lock.modules.insert(
            "terminal".to_string(),
            LockEntry {
                name: "terminal".to_string(),
                version: "1.0.0".to_string(),
                source: "https://example.org/release".to_string(),
                asset: "terminal-linux-x86_64.so".to_string(),
                sha256: "ab".repeat(32),
                global: false,
            },
        );
        let text = lock.render();
        let back = Lockfile::parse(&text).expect("rendered lockfile parses");
        assert_eq!(lock, back);
        // A missing `global` key reads as false — older lockfiles stay valid.
        let minimal =
            r#"{"modules":{"a":{"name":"a","version":"1","source":"s","asset":"f","sha256":"h"}}}"#;
        let parsed = Lockfile::parse(minimal).unwrap();
        assert!(!parsed.modules["a"].global);
    }

    #[test]
    fn release_asset_base_maps_tag_to_download() {
        let url = "https://github.com/o/r/releases/tag/modules/terminal/v1.0.0";
        assert_eq!(
            release_asset_base(url).unwrap(),
            "https://github.com/o/r/releases/download/modules/terminal/v1.0.0"
        );
        // Trailing slashes are tolerated.
        assert_eq!(
            release_asset_base(&format!("{url}/")).unwrap(),
            release_asset_base(url).unwrap()
        );
        assert!(release_asset_base("https://example.org/x.json").is_err());
    }

    #[test]
    fn manifest_url_for_distinguishes_pages_from_direct_urls() {
        let tag_page = "https://github.com/o/r/releases/tag/v1.0.0";
        assert_eq!(
            manifest_url_for(tag_page, "terminal"),
            "https://github.com/o/r/releases/tag/v1.0.0/terminal.json"
        );
        // A download URL pointing straight at the manifest is itself the
        // manifest — appending `{name}.json` to it would 404.
        let direct = "https://github.com/o/r/releases/download/v1.0.0/terminal.json";
        assert_eq!(manifest_url_for(direct, "terminal"), direct);
    }

    #[test]
    fn find_project_code_dir_walks_up_and_stops_at_nearest() {
        let tmp = std::env::temp_dir().join(format!("code-install-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let outer = tmp.join("outer");
        let inner = outer.join("mid").join("deep");
        fs::create_dir_all(inner.join(".code")).unwrap();
        fs::create_dir_all(tmp.join("other").join(".code")).unwrap();

        // From deep inside, the nearest `.code` wins over any ancestor's.
        assert_eq!(
            crate::loader::find_project_code_dir(&inner),
            Some(inner.join(".code"))
        );
        // Walking up only ever finds ancestors' `.code` dirs — `inner`'s is
        // invisible from `outer`, which sits above it.
        assert_eq!(crate::loader::find_project_code_dir(&outer), None);
        // Unrelated trees do not leak in.
        assert_eq!(
            crate::loader::find_project_code_dir(&tmp.join("other")),
            Some(tmp.join("other").join(".code"))
        );
        assert_eq!(
            crate::loader::find_project_code_dir(&tmp.join("elsewhere")),
            None
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ensure_project_code_dir_creates_on_demand() {
        let tmp =
            std::env::temp_dir().join(format!("code-install-test-bare-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let created = ensure_project_code_dir(&tmp).unwrap();
        assert_eq!(created, tmp.join(".code"));
        assert!(created.is_dir());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn verify_sha256_checks_real_files() {
        let tmp =
            std::env::temp_dir().join(format!("code-install-test-sha-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let f = tmp.join("payload.bin");
        fs::write(&f, b"hello, module").unwrap();
        let digest = sha256_of(&f).unwrap();
        assert_eq!(digest.len(), 64);
        assert!(verify_sha256(&f, &digest).unwrap());
        assert!(
            verify_sha256(&f, &digest.to_uppercase()).unwrap(),
            "hex case-insensitive"
        );
        assert!(!verify_sha256(&f, &"0".repeat(64)).unwrap());
        let _ = fs::remove_dir_all(&tmp);
    }
}

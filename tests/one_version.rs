//! Everything this repository publishes carries one version number.
//!
//! The CLI, the four-and-counting native modules, `code-native` on crates.io
//! and the `code-wasm` npm package used to version independently — each on
//! its own tag, on its own cadence. That was tidy for whoever cut the release
//! and useless for whoever consumed it: a user holding `code v0.7.0` and
//! `terminal 1.0.0` had no way to know whether the two were built against the
//! same ABI without reading the repo.
//!
//! So they move together, and this test is what makes that true rather than
//! intended. It reads the version out of every manifest that gets published
//! and refuses any that disagree — which is also what lets a release be one
//! tag rather than six.
//!
//! Not the same thing as `CODE_ABI_VERSION`, which stays where it is: that
//! says whether a module *can* be loaded, and it moves only when the ABI
//! actually breaks. This says which release something came from.

use std::fs;
use std::path::{Path, PathBuf};

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

/// The first `version = "..."` in a Cargo.toml — the package's own, since it
/// precedes every dependency's.
fn cargo_version(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    text.lines()
        .find_map(|line| {
            let rest = line.trim().strip_prefix("version")?.trim_start();
            let rest = rest.strip_prefix('=')?.trim();
            rest.strip_prefix('"')?.split('"').next().map(str::to_owned)
        })
        .unwrap_or_else(|| panic!("no version in {}", path.display()))
}

fn json_version(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let value: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()));
    value["version"]
        .as_str()
        .unwrap_or_else(|| panic!("no version in {}", path.display()))
        .to_string()
}

#[test]
fn every_published_artifact_shares_the_repository_version() {
    let expected = env!("CARGO_PKG_VERSION");

    let mut manifests: Vec<PathBuf> = vec![
        repo("crates/code-native/Cargo.toml"),
        repo("crates/code-wasm/Cargo.toml"),
        repo("crates/code-lsp/Cargo.toml"),
    ];
    // Every module under crates/modules/, found rather than listed: a module
    // added next month is published too, and forgetting it here would be the
    // exact drift this test exists to stop.
    let modules = fs::read_dir(repo("crates/modules")).expect("read crates/modules");
    for entry in modules {
        let path = entry.expect("read crates/modules entry").path();
        if path.join("Cargo.toml").is_file() {
            manifests.push(path.join("Cargo.toml"));
        }
    }
    assert!(
        manifests.len() >= 5,
        "expected the crates plus every module, found {}",
        manifests.len()
    );

    let mut wrong = Vec::new();
    for manifest in &manifests {
        let found = cargo_version(manifest);
        if found != expected {
            wrong.push(format!("{} is {found}", manifest.display()));
        }
    }
    let npm = repo("crates/code-wasm/npm/package.json");
    let found = json_version(&npm);
    if found != expected {
        wrong.push(format!("{} is {found}", npm.display()));
    }

    assert!(
        wrong.is_empty(),
        "the repository is at {expected} and these are not:\n  {}\n\
         Everything published from here shares one version so a user never has \
         to ask whether their `code` and their modules match.",
        wrong.join("\n  ")
    );
}

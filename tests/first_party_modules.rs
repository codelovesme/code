//! One list of first-party modules, in three places that must agree.
//!
//! `code install <name>` used to ask a `modules-index.json` served from the
//! Pages site which modules exist and what their latest version was. Once one
//! `v*` tag started releasing the CLI and every module together (see
//! `one_version.rs`), that index had nothing left to answer — a name plus the
//! binary's own version already *is* the address — and it went. What replaced
//! it is a list compiled into the binary, `module_install::FIRST_PARTY`.
//!
//! A hand-maintained list is exactly what the index was, though, and the index
//! ended its life missing two of the six modules: `env` and `http_server` were
//! written, built and published without anyone editing it, so `code install
//! env` answered "unknown module". So the list is held here to the two other
//! places that enumerate the same set — the `crates/modules/` directory, which
//! is the ground truth, and the publish workflow's build matrix, which decides
//! what actually reaches a release.
//!
//! The failure this stops is never a broken build; it is a module that exists
//! and cannot be installed.

use std::fs;
use std::path::{Path, PathBuf};

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

/// Every directory under `crates/modules/` that is a crate — the modules that
/// exist, found rather than listed.
fn modules_on_disk() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(repo("crates/modules"))
        .expect("read crates/modules")
        .map(|entry| entry.expect("read crates/modules entry").path())
        .filter(|path| path.join("Cargo.toml").is_file())
        .filter_map(|path| path.file_name()?.to_str().map(str::to_owned))
        .collect();
    names.sort();
    names
}

/// Every `module: [a, b, c]` matrix in the publish workflow. There is one per
/// job (build, dogfood); both must cover every module or a release ships an
/// artifact nothing proved, or proves an artifact it never built.
fn workflow_matrices() -> Vec<Vec<String>> {
    let text = fs::read_to_string(repo(".github/workflows/publish-modules.yml"))
        .expect("read publish-modules.yml");
    text.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("module: [")?;
            let inner = rest.strip_suffix(']')?;
            let mut names: Vec<String> = inner
                .split(',')
                .map(|name| name.trim().to_owned())
                .filter(|name| !name.is_empty())
                .collect();
            names.sort();
            Some(names)
        })
        .collect()
}

#[test]
fn the_compiled_in_list_is_every_module_in_the_tree() {
    let mut listed: Vec<String> = code::module_install::FIRST_PARTY
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    listed.sort();

    assert_eq!(
        listed,
        modules_on_disk(),
        "`module_install::FIRST_PARTY` and `crates/modules/` disagree. A module \
         missing from the list cannot be installed by name; a name in the list \
         with no module behind it resolves to a 404. Add it in both."
    );
}

#[test]
fn the_publish_workflow_builds_every_module() {
    let on_disk = modules_on_disk();
    let matrices = workflow_matrices();

    assert!(
        matrices.len() >= 2,
        "expected a `module:` matrix in both the build and dogfood jobs of \
         publish-modules.yml, found {}",
        matrices.len()
    );
    for matrix in &matrices {
        assert_eq!(
            matrix, &on_disk,
            "a `module:` matrix in publish-modules.yml does not match \
             `crates/modules/` — a module left out of it is never built, \
             dogfooded or attached to the release, so installing it by name \
             fails against a release that does not carry it."
        );
    }
}

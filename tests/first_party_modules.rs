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
//! is the ground truth, and the release plan (`scripts/module-hashes.sh`,
//! which every matrix in the publish workflow takes its modules from), which
//! decides what actually reaches a release.
//!
//! The failure this stops is never a broken build; it is a module that exists
//! and cannot be installed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Every `module:` matrix in the publish workflow, with the job it belongs
/// to.
fn workflow_matrices() -> Vec<(String, String)> {
    let text = fs::read_to_string(repo(".github/workflows/publish-modules.yml"))
        .expect("read publish-modules.yml");
    let mut job = String::new();
    let mut found = Vec::new();
    for line in text.lines() {
        // A job header is the only thing indented exactly two spaces and
        // ending in a colon — enough to say which job a matrix belongs to.
        if let Some(name) = line.strip_prefix("  ") {
            if !name.starts_with(' ') && name.ends_with(':') && !name.contains(' ') {
                job = name.trim_end_matches(':').to_owned();
            }
        }
        if let Some(rest) = line.trim().strip_prefix("module: ") {
            found.push((job.clone(), rest.to_owned()));
        }
    }
    found
}

/// What `scripts/module-hashes.sh` plans a release from: each module's name
/// and whether it has a browser half.
fn release_plan() -> Vec<(String, bool)> {
    let output = Command::new("bash")
        .arg(repo("scripts/module-hashes.sh"))
        .output()
        .expect("run scripts/module-hashes.sh");
    assert!(
        output.status.success(),
        "scripts/module-hashes.sh failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut plan: Vec<(String, bool)> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            let fields: Vec<&str> = line.split(' ').collect();
            assert_eq!(
                fields.len(),
                3,
                "unexpected line from module-hashes.sh: {line}"
            );
            (fields[0].to_owned(), fields[2] == "1")
        })
        .collect();
    plan.sort();
    plan
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
fn the_release_plan_covers_every_module() {
    let plan = release_plan();
    let names: Vec<String> = plan.iter().map(|(name, _)| name.clone()).collect();
    assert_eq!(
        names,
        modules_on_disk(),
        "scripts/module-hashes.sh and `crates/modules/` disagree — a module the \
         release plan does not list is never built, reused, dogfooded or \
         attached to the release, so installing it by name fails."
    );

    // The browser's half: a module with a page.mjs is a browser module, and
    // the plan is what sends it to the wasm32 build. That is how `media`
    // shipped in 2.6.0 with no `media-wasm32.a` — back when the wasm32 list
    // was typed out by hand, a module missing from it still passed.
    for (name, wasm) in &plan {
        let half = repo("crates/modules").join(name).join("page.mjs");
        assert_eq!(
            *wasm,
            half.is_file(),
            "the release plan says {name} {} a browser half, but crates/modules/{name}/page.mjs {}",
            if *wasm { "has" } else { "has no" },
            if half.is_file() { "exists" } else { "does not" }
        );
    }
}

#[test]
fn the_publish_workflow_takes_its_modules_from_the_plan() {
    // Every matrix comes from `plan`, which lists crates/modules/ itself. A
    // module list typed into the workflow is the drift this file exists to
    // stop: it is how `env` and `http_server` once shipped uninstallable.
    let expected = [
        ("build-linux-x86_64", "build"),
        ("build-wasm32", "build_wasm"),
        ("reuse", "reuse"),
        ("dogfood", "all"),
    ];
    let matrices = workflow_matrices();
    for (job, output) in expected {
        let want = format!("${{{{ fromJSON(needs.plan.outputs.{output}) }}}}");
        assert!(
            matrices.iter().any(|(j, m)| j == job && *m == want),
            "publish-modules.yml's `{job}` job should take `module: {want}`; found {:?}",
            matrices
                .iter()
                .filter(|(j, _)| j == job)
                .collect::<Vec<_>>()
        );
    }
    for (job, matrix) in &matrices {
        assert!(
            matrix.starts_with("${{ fromJSON(needs.plan.outputs."),
            "publish-modules.yml's `{job}` job lists its modules by hand ({matrix}); \
             take them from the `plan` job instead"
        );
    }
}

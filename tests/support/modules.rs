//! Building a module crate for a test, into one target directory shared by
//! every module.
//!
//! Each module is its own standalone workspace (see any
//! `crates/modules/*/Cargo.toml`), so left alone each one built into its own
//! `target/` — and compiled its own copy of `code-native`, serde, rustls and
//! the rest. Forty-odd modules meant forty-odd copies: 11 GB on disk, and a
//! one-line change to `code-native` rebuilt the lot one module at a time
//! (210s). Pointing every build at `target/modules` keeps the modules
//! standalone — each still resolves against its own `Cargo.lock`, exactly as
//! the release workflow builds it — while a dependency built once is built
//! for all of them (1.1 GB, 28s for the same change).
//!
//! Its own directory rather than the main `target/`: `cargo test` holds the
//! lock on that one while the tests run, so a nested build there would wait
//! on itself. Builds that overlap — two test binaries asking for the same
//! module — are serialised by cargo's lock on this one.
//!
//! Pulled into each test binary with `#[path = "support/modules.rs"] mod
//! modules;` — a file under `tests/support/` is not a test target itself.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The repository root.
pub fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Where every module build lands.
pub fn target_dir() -> PathBuf {
    repo().join("target/modules")
}

/// A module crate's directory: the `test_*` doubles live beside the fixtures
/// in `tests/native_modules/`, the shipped modules under `crates/modules/`.
pub fn crate_dir(stem: &str) -> PathBuf {
    if stem.starts_with("test_") {
        repo().join("tests/native_modules").join(stem)
    } else {
        repo().join("crates/modules").join(stem)
    }
}

/// `cargo <args>` inside `stem`'s crate, into the shared target directory.
/// Panics with cargo's own output when the build fails.
pub fn cargo(stem: &str, args: &[&str]) {
    let output = Command::new("cargo")
        .args(args)
        .current_dir(crate_dir(stem))
        .env("CARGO_TARGET_DIR", target_dir())
        .output()
        .unwrap_or_else(|e| panic!("failed to run cargo for {stem}: {e}"));
    assert!(
        output.status.success(),
        "cargo failed to build {stem}:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Builds a module and returns its `.so`.
///
/// The path is the shared one, which the next build of the same module
/// overwrites — copy it somewhere of your own before handing it to a
/// program that outlives the call.
pub fn build_so(stem: &str) -> PathBuf {
    cargo(stem, &["build", "--release"]);
    target_dir().join(format!("release/lib{stem}.so"))
}

/// Builds a `staticlib` crate (the `test_*_static` doubles) and returns its
/// `.a`.
pub fn build_a(stem: &str) -> PathBuf {
    cargo(stem, &["build", "--release"]);
    target_dir().join(format!("release/lib{stem}.a"))
}

/// Builds a module's browser half and returns its `.a`. A `cdylib` is a
/// whole module and fails to link on the very imports an archive is supposed
/// to leave open, so the crate type is asked for on the command line — the
/// same line the release workflow runs.
pub fn build_wasm32_a(stem: &str) -> PathBuf {
    cargo(
        stem,
        &[
            "rustc",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
            "--crate-type",
            "staticlib",
        ],
    );
    target_dir().join(format!("wasm32-unknown-unknown/release/lib{stem}.a"))
}

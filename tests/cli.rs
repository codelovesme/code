//! The CLI's shape: what `run`/`build` do with a file against a directory,
//! and what asking for help answers.
//!
//! One command each, taking either: a file is itself, a directory is its
//! `main.code`. The kind decides one thing only — where `build` writes, which
//! is beside the source for a file and `build/` for a project — so the tests
//! are about which of those two rules applied.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("code-app-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    dir
}

fn code(dir: &PathBuf, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_code"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run code")
}

#[test]
fn run_finds_a_projects_entry_point() {
    let dir = temp_dir("run");
    assert!(code(&dir, &["init", "demo"]).status.success());

    let out = code(&dir, &["run", "demo"]);
    assert!(
        out.status.success(),
        "code run failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // No directory at all means the one you are standing in.
    let project = dir.join("demo");
    assert!(code(&project, &["run"]).status.success());

    // A directory with no main.code says so, rather than reporting a missing
    // file the user never named.
    let empty = dir.join("empty");
    fs::create_dir_all(&empty).expect("create empty dir");
    let out = code(&dir, &["run", "empty"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("main.code"),
        "the error should name the entry point: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let _ = fs::remove_dir_all(&dir);
}

#[cfg(feature = "llvm")]
#[test]
fn building_a_directory_writes_into_build() {
    let dir = temp_dir("build");
    assert!(code(&dir, &["init", "demo"]).status.success());

    let out = code(&dir, &["build", "demo"]);
    assert!(
        out.status.success(),
        "code build failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Named for the project, not for `main` — every app's entry file has the
    // same name, so `build/main` would say nothing.
    let artifact = dir.join("demo/build/demo");
    assert!(
        artifact.is_file(),
        "expected demo/build/demo, found: {:?}",
        fs::read_dir(dir.join("demo/build")).map(|d| d.count())
    );
    assert!(
        Command::new(&artifact)
            .status()
            .expect("run the artifact")
            .success(),
        "the built app did not run"
    );

    // `.` has no name of its own, so the project name comes from the
    // filesystem rather than from the argument.
    let project = dir.join("demo");
    assert!(code(&project, &["build", "."]).status.success());
    assert!(project.join("build/demo").is_file());

    // `-o` still wins, and takes the artifact out of `build/` entirely.
    assert!(code(&project, &["build", ".", "-o", "elsewhere"])
        .status
        .success());
    assert!(project.join("elsewhere").is_file());

    // `--output` is the same flag spelled long.
    assert!(code(&project, &["build", ".", "--output", "longform"])
        .status
        .success());
    assert!(project.join("longform").is_file());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn help_is_an_answer_rather_than_an_error() {
    let dir = temp_dir("help");

    // Asking is never a failure: stdout, exit 0.
    for args in [&["--help"][..], &["-h"][..], &["help"][..]] {
        let out = code(&dir, args);
        assert!(out.status.success(), "`code {args:?}` failed");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("code — the Code programming language"));
        assert!(text.contains("init [name]"), "help omits app: {text}");
        assert!(
            text.contains("install <name-or-url>"),
            "help omits module: {text}"
        );
    }

    // Per-command help, both spellings — and after the command, since a help
    // flag should not have to be in the right position.
    for args in [
        &["build", "--help"][..],
        &["help", "build"][..],
        &["build", "x.code", "-h"][..],
    ] {
        let out = code(&dir, args);
        assert!(out.status.success());
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains("usage: code build [path]"),
            "expected build's help for {args:?}, got: {text}"
        );
    }

    // No command at all is still a usage error, and an unknown one says where
    // to look rather than dumping everything.
    let out = code(&dir, &[]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage:"));

    let out = code(&dir, &["bogus"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("code --help"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn version_answers_to_all_three_spellings() {
    let dir = temp_dir("version");
    for args in [&["--version"][..], &["-v"][..], &["version"][..]] {
        let out = code(&dir, args);
        assert!(out.status.success(), "`code {args:?}` failed");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.starts_with("Code v"),
            "expected a version line for {args:?}, got: {text}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

//! `code app run|build [dir]` — the directory-shaped half of the CLI.
//!
//! `run`/`build` take a file and answer beside it; `app run`/`app build` take
//! a project, find its `main.code`, and put artifacts in `build/`. Two
//! commands rather than one that guesses, so the tests are about which of
//! the two rules applied.

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
fn app_run_finds_the_entry_point() {
    let dir = temp_dir("run");
    assert!(code(&dir, &["init", "demo"]).status.success());

    let out = code(&dir, &["app", "run", "demo"]);
    assert!(
        out.status.success(),
        "code app run failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // No directory at all means the one you are standing in.
    let project = dir.join("demo");
    assert!(code(&project, &["app", "run"]).status.success());

    // A directory with no main.code says so, rather than reporting a missing
    // file the user never named.
    let empty = dir.join("empty");
    fs::create_dir_all(&empty).expect("create empty dir");
    let out = code(&dir, &["app", "run", "empty"]);
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
fn app_build_writes_into_build_named_after_the_project() {
    let dir = temp_dir("build");
    assert!(code(&dir, &["init", "demo"]).status.success());

    let out = code(&dir, &["app", "build", "demo"]);
    assert!(
        out.status.success(),
        "code app build failed: {}",
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
    assert!(code(&project, &["app", "build", "."]).status.success());
    assert!(project.join("build/demo").is_file());

    // `-o` still wins, and takes the artifact out of `build/` entirely.
    assert!(code(&project, &["app", "build", ".", "-o", "elsewhere"])
        .status
        .success());
    assert!(project.join("elsewhere").is_file());

    let _ = fs::remove_dir_all(&dir);
}

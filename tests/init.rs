//! `code init` — the scaffold has to run before anything is installed.
//!
//! The property worth testing is not which files appear but that the program
//! it writes *works with nothing installed*: the obvious template prints
//! something, printing needs the `terminal` module, and a new project whose
//! first act is a failed `link` is a bad first minute. So this runs the
//! generated `main.code` for real.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("code-init-{tag}-{}", std::process::id()));
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
fn the_scaffolded_program_runs_with_nothing_installed() {
    let dir = temp_dir("runs");
    let out = code(&dir, &["init", "demo"]);
    assert!(out.status.success(), "code init failed: {out:?}");

    let project = dir.join("demo");
    for expected in ["main.code", ".gitignore"] {
        assert!(
            project.join(expected).is_file(),
            "expected {expected} in the scaffold"
        );
    }
    // `.code/` is what marks the project root for module resolution
    // (`loader::find_project_code_dir`), which is the whole reason an empty
    // lockfile is worth writing.
    assert!(project.join(".code/lock.json").is_file());

    let run = code(&project, &["run", "main.code"]);
    assert!(
        run.status.success(),
        "the scaffolded program did not run: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    // And it is already in the canonical layout, so a project that adopts the
    // CI gate on day one passes it on day one.
    let formatted = code(&project, &["format", "--check", "main.code"]);
    assert!(
        formatted.status.success(),
        "the template is not canonically formatted: {}",
        String::from_utf8_lossy(&formatted.stdout)
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_existing_file_is_a_refusal_not_a_merge() {
    let dir = temp_dir("refuses");
    fs::write(dir.join("main.code"), "let mine = 1\n").expect("write main.code");

    let out = code(&dir, &["init"]);
    assert!(!out.status.success(), "init overwrote an existing project");
    assert_eq!(
        fs::read_to_string(dir.join("main.code")).expect("read main.code"),
        "let mine = 1\n",
        "init touched a file it should have refused"
    );
    // Refused before writing anything, so nothing half-made is left behind.
    assert!(!dir.join(".code").exists());

    let _ = fs::remove_dir_all(&dir);
}

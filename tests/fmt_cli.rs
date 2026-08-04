//! End-to-end tests for the `code fmt` CLI command.

use std::fs;
use std::process::Command;

fn tmp(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("code_fmt_{}_{}.code", std::process::id(), name))
}

#[test]
fn fmt_check_then_format_in_place() {
    let exe = env!("CARGO_BIN_EXE_code");
    let path = tmp("inplace");
    fs::write(&path, "loop {\nif x {\nyield x\n}\n}\n").unwrap();
    let p = path.to_str().unwrap();

    // --check on an unformatted file fails (nonzero exit) without modifying it.
    let st = Command::new(exe).args(["fmt", p, "--check"]).status().unwrap();
    assert!(!st.success(), "--check should fail on an unformatted file");
    assert_eq!(fs::read_to_string(&path).unwrap(), "loop {\nif x {\nyield x\n}\n}\n");

    // Formatting in place rewrites with 4-space indentation.
    let st = Command::new(exe).args(["fmt", p]).status().unwrap();
    assert!(st.success());
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "loop {\n    if x {\n        yield x\n    }\n}\n",
    );

    // --check now passes on the formatted file.
    let st = Command::new(exe).args(["fmt", p, "--check"]).status().unwrap();
    assert!(st.success(), "--check should pass on a formatted file");

    let _ = fs::remove_file(&path);
}

#[test]
fn fmt_directory_recurses() {
    let exe = env!("CARGO_BIN_EXE_code");
    let dir = std::env::temp_dir().join(format!("code_fmt_dir_{}", std::process::id()));
    let nested = dir.join("nested");
    fs::create_dir_all(&nested).unwrap();
    let messy = nested.join("m.code");
    fs::write(&messy, "loop {\nif x {\nyield x\n}\n}\n").unwrap();
    fs::write(dir.join("ok.code"), "a = 1\n").unwrap(); // already formatted
    let d = dir.to_str().unwrap();

    // --check fails because a nested file is unformatted.
    let st = Command::new(exe).args(["fmt", d, "--check"]).status().unwrap();
    assert!(!st.success(), "--check on a dir with unformatted files should fail");

    // Formatting the directory fixes the nested file.
    let st = Command::new(exe).args(["fmt", d]).status().unwrap();
    assert!(st.success());
    assert_eq!(
        fs::read_to_string(&messy).unwrap(),
        "loop {\n    if x {\n        yield x\n    }\n}\n",
    );

    // --check now passes for the whole directory.
    let st = Command::new(exe).args(["fmt", d, "--check"]).status().unwrap();
    assert!(st.success(), "--check should pass once the directory is formatted");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fmt_missing_file_argument_errors() {
    let exe = env!("CARGO_BIN_EXE_code");
    let st = Command::new(exe).arg("fmt").status().unwrap();
    assert!(!st.success(), "`code fmt` with no file should error");
}

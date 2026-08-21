use std::process::Command;

#[test]
fn run_link_basic() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["run", "tests/link_basic.code"])
        .status()
        .expect("failed to run code");
    assert!(status.success());
}

#[test]
fn run_link_alias() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["run", "tests/link_alias.code"])
        .status()
        .expect("failed to run code");
    assert!(status.success());
}

#[test]
fn run_link_export_trailing() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["run", "tests/link_export_trailing.code"])
        .status()
        .expect("failed to run code");
    assert!(status.success());
}

#[test]
fn run_link_flatten_private() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["run", "tests/link_flatten_private.code"])
        .status()
        .expect("failed to run code");
    assert!(status.success());
}

#[test]
fn run_link_missing_fails() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["run", "tests/fail_link_missing.code"])
        .status()
        .expect("failed to run code");
    assert!(!status.success());
}

#[test]
fn run_link_circular_fails() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["run", "tests/fail_link_cycle.code"])
        .status()
        .expect("failed to run code");
    assert!(!status.success());
}

#[test]
fn run_link_private_alias_fails() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["run", "tests/fail_link_no_export.code"])
        .status()
        .expect("failed to run code");
    assert!(!status.success());
}

#[test]
fn run_link_private_flatten_fails() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["run", "tests/fail_link_private_flatten.code"])
        .status()
        .expect("failed to run code");
    assert!(!status.success());
}

#[test]
fn run_link_duplicate_fails() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["run", "tests/fail_link_duplicate.code"])
        .status()
        .expect("failed to run code");
    assert!(!status.success());
}

#[test]
fn build_link_emits_ir() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["build", "tests/link_basic.code", "--target", "ir"])
        .status()
        .expect("failed to run code build");
    assert!(status.success());

    let ir = std::fs::read_to_string("target/llvm/link_basic.ll")
        .expect("missing IR output");
    assert!(ir.contains("define i32 @main"));
}

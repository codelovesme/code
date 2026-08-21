use std::process::Command;

#[test]
fn run_json_module() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["run", "tests/json_module.code"])
        .status()
        .expect("failed to run code");
    assert!(status.success());
}

//! The `env` module against variables that are actually set.
//!
//! `env_basic.code` covers what can be asserted with nothing arranged — an
//! absent variable, a default, `Require` refusing. Everything that needs a
//! variable to *exist* lives here instead, because a fixture cannot set one
//! for itself.
//!
//! Both output modes, because a module is where `code run` and `code build`
//! differ most.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn build_module() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/env");
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        .status()
        .expect("run cargo for crates/modules/env");
    assert!(status.success(), "cargo failed to build env");
    crate_dir.join("target/release/libenv.so")
}

/// Writes `program` into a private directory beside a fresh copy of the
/// module and runs it both ways with `vars` set. Passing on both is the
/// assertion — the programs `assert` for themselves.
fn run_both_ways(tag: &str, program: &str, vars: &[(&str, &str)], expect_success: bool) {
    let dir = std::env::temp_dir().join(format!("code-env-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    fs::copy(build_module(), dir.join("env.so")).expect("copy env.so");
    let source = dir.join("main.code");
    fs::write(&source, program).expect("write program");

    let interpreted = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("run")
        .arg(&source)
        .current_dir(&dir)
        .envs(vars.iter().copied())
        .status()
        .expect("spawn code run");
    assert_eq!(
        interpreted.success(),
        expect_success,
        "code run disagreed for {tag}"
    );

    let exe = dir.join("main");
    code::compile_file(&source, code::BuildTarget::Exe, &exe, false).expect("compile");
    let compiled = Command::new(&exe)
        .current_dir(&dir)
        .envs(vars.iter().copied())
        .env("CODE_CHECK_LEAKS", "1")
        .status()
        .expect("run the compiled program");
    assert_eq!(
        compiled.success(),
        expect_success,
        "code build disagreed for {tag}"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_default_decides_how_a_set_variable_is_read() {
    // One emit, not two: the port arrives as a Number because 8080 is one.
    // Without this the program would have to parse a string, and the language
    // has no way to.
    run_both_ways(
        "kinds",
        r#"link "env.so" as env

emit Get { name = "CODE_TEST_PORT", default = 8080 } to env get port
assert port.found
assert port.value = 9090

emit Get { name = "CODE_TEST_DEBUG", default = false } to env get debug
assert debug.value

| No default: the string as it stands.
emit Get { name = "CODE_TEST_NAME" } to env get name
assert name.value = "ada"

| A default that is a string does not turn the value into anything else.
emit Get { name = "CODE_TEST_PORT", default = "none" } to env get text
assert text.value = "9090"

| Set, so `Require` is satisfied.
emit Require { name = "CODE_TEST_NAME" } to env get needed
assert needed.value = "ada"
"#,
        &[
            ("CODE_TEST_PORT", "9090"),
            ("CODE_TEST_DEBUG", "true"),
            ("CODE_TEST_NAME", "ada"),
        ],
        true,
    );
}

#[test]
fn a_variable_that_cannot_be_read_as_its_default_is_an_exception() {
    // Not a silent fallback to 8080: `PORT=banana` is a deployment mistake,
    // and listening on the wrong port instead would hide it until someone
    // wondered why nothing was arriving.
    run_both_ways(
        "unparseable",
        r#"link "env.so" as env

emit Get { name = "CODE_TEST_PORT", default = 8080 } to env get port
assert port ∈ Exception
assert port.source = "env"

| And the program is still running, which is the whole point of an
| Exception being a value.
assert 1 + 1 = 2
"#,
        &[("CODE_TEST_PORT", "banana")],
        true,
    );
}

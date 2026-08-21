//! Discovers and runs every `tests/*.code` fixture through BOTH output
//! modes — `code::run_source` (interpret) and `code::compile_source` (LLVM
//! compile + link + execute) — since the language is meant to run every
//! feature identically either way (see memory `new-language-rewrite`).
//! This file is wiring only — the tests themselves are the `.code` files.
//!
//! Pass criterion for a plain `foo.code`: both modes must succeed, and the
//! compiled binary's stdout must match the interpreter's bindings dump
//! exactly. For a `fail_foo.code`: both modes must produce an error (a
//! parse/undefined-variable error is caught identically by the interpreter
//! at eval time and by the compiler's compile-time `verify_defined` pass).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn code_fixtures_run_as_expected() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let tmp_dir = std::env::temp_dir().join("code-compiler-tests");
    fs::create_dir_all(&tmp_dir).expect("create temp dir for compiled fixtures");

    let mut failures = Vec::new();
    let mut checked = 0;

    for entry in fs::read_dir(&dir).expect("read tests/ directory") {
        let path = entry.expect("read tests/ directory entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("code") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let src = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {name}: {e}"));
        let should_fail = name.starts_with("fail_");

        check_interpret(&name, &src, should_fail, &mut failures);
        check_compile(&name, &src, should_fail, &tmp_dir, &mut failures);
        checked += 1;
    }

    assert!(checked > 0, "no .code fixtures found in {}", dir.display());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn check_interpret(name: &str, src: &str, should_fail: bool, failures: &mut Vec<String>) {
    match (should_fail, code::run_source(src)) {
        (false, Err(e)) => failures.push(format!(
            "{name}: interpret: expected to run, but errored: {e}"
        )),
        (true, Ok(_)) => failures.push(format!("{name}: interpret: expected an error, but it ran")),
        _ => {}
    }
}

fn check_compile(
    name: &str,
    src: &str,
    should_fail: bool,
    tmp_dir: &Path,
    failures: &mut Vec<String>,
) {
    let stem = name.trim_end_matches(".code");
    let exe_path: PathBuf = tmp_dir.join(stem);

    let compile_result = code::compile_source(src, &exe_path);

    match (should_fail, compile_result) {
        (false, Err(e)) => {
            failures.push(format!(
                "{name}: compile: expected to compile, but errored: {e}"
            ));
        }
        (true, Ok(())) => {
            failures.push(format!(
                "{name}: compile: expected an error, but it compiled"
            ));
        }
        (false, Ok(())) => {
            let output = Command::new(&exe_path)
                .output()
                .unwrap_or_else(|e| panic!("{name}: run compiled binary: {e}"));
            let compiled_stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let interpreted_stdout = code::run_source(src)
                .map(|env| code::format_bindings(&env))
                .unwrap_or_default();
            if compiled_stdout != interpreted_stdout {
                failures.push(format!(
                    "{name}: compile: stdout mismatch\n  interpreted: {interpreted_stdout:?}\n  compiled:    {compiled_stdout:?}"
                ));
            }
            let _ = fs::remove_file(&exe_path);
        }
        (true, Err(_)) => {}
    }
}

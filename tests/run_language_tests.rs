//! Discovers and runs every `tests/*.code` fixture through BOTH output
//! modes — `code::run_source` (interpret) and `code::compile_source` (LLVM
//! compile + link + execute) — since the language is meant to run every
//! feature identically either way (see memory `new-language-rewrite`).
//! This file is wiring only — the tests themselves are the `.code` files.
//!
//! Pass criterion for a plain `foo.code`: both modes must succeed, and the
//! compiled binary's stdout must match the interpreter's bindings dump
//! exactly. For a `fail_foo.code`: both modes must produce an error — for
//! the interpreter that's always `run_source` returning `Err`; for the
//! compiler it's either a compile-time error (`compile_source` returning
//! `Err`, e.g. a parse error or `verify_defined`'s undefined-variable
//! check) OR the compiled binary itself exiting non-zero at runtime (e.g. a
//! type mismatch or division by zero — those operand types aren't known
//! until the program actually runs, so the compiled binary has to detect
//! and report them itself; see `runtime.c`'s `code_runtime_error`).

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

    match code::compile_source(src, &exe_path) {
        Err(e) => {
            // A compile-time failure (parse error, undefined variable) is a
            // valid way for a fail_*.code fixture to fail — nothing further
            // to check either way.
            if !should_fail {
                failures.push(format!(
                    "{name}: compile: expected to compile, but errored: {e}"
                ));
            }
        }
        Ok(()) => {
            let output = Command::new(&exe_path)
                .output()
                .unwrap_or_else(|e| panic!("{name}: run compiled binary: {e}"));

            if should_fail {
                if output.status.success() {
                    failures.push(format!(
                        "{name}: compile: expected an error (compile-time or at runtime), but the binary ran successfully"
                    ));
                }
                // A non-zero exit is exactly what a fail_*.code fixture
                // whose error only exists at runtime (a type mismatch,
                // division by zero) is expected to produce — nothing
                // further to check.
            } else if !output.status.success() {
                failures.push(format!(
                    "{name}: compile: expected the binary to run cleanly, but it exited with {}; stderr: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                ));
            } else {
                let compiled_stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let interpreted_stdout = code::run_source(src)
                    .map(|env| code::format_bindings(&env))
                    .unwrap_or_default();
                if compiled_stdout != interpreted_stdout {
                    failures.push(format!(
                        "{name}: compile: stdout mismatch\n  interpreted: {interpreted_stdout:?}\n  compiled:    {compiled_stdout:?}"
                    ));
                }
            }
            let _ = fs::remove_file(&exe_path);
        }
    }
}

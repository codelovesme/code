//! The two backends must report the same failure identically — message and
//! source location both.
//!
//! This is not a style rule. Since phase 2 a module's failure comes back as
//! `Exception { source, message, innerException }`, and since phase 4 the
//! language's own failures do too — so `message` is an ordinary string a
//! program can read, compare, print, or return. Two backends disagreeing
//! about it is a difference in what a program *computes*, not in what a user
//! sees on stderr.
//!
//! Nothing else catches it. `run_language_tests.rs` compares only pass/fail,
//! and a fixture that asserted a message would simply fail in one mode
//! instead of reporting a divergence. Module messages are safe by
//! construction — the same `.so` runs under both modes — so everything here
//! is a *language-level* failure, which is the only kind with two independent
//! implementations: `interpreter.rs` in Rust and `runtime.c` in C.
//!
//! Before this existed, seven of nine message pairs had drifted. The two that
//! had not were exactly the two carrying a "must match interpreter.rs
//! exactly" comment — the discipline worked, it just had no way to be
//! enforced.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Every language-level runtime failure that produces a message, one per
/// site in `runtime.c` that can `fail`. Compile-time refusals (an undefined
/// name, a parse error) are deliberately absent: they never reach a runtime
/// at all, so there is only one implementation to disagree with.
///
/// Each program is written to fail at the *top level*, where the message
/// reaches stderr in both modes. The same text is what a handler-level
/// failure puts in `Exception.message` — it comes from the same `fail` call.
const CASES: &[(&str, &str)] = &[
    ("add", "let x = 1 + true\n"),
    ("sub", "let x = 1 - \"a\"\n"),
    ("mul", "let x = true * 2\n"),
    ("div", "let x = 1 / \"a\"\n"),
    ("div_zero", "let n = 0\nlet x = 1 / n\n"),
    ("compare_lt", "let x = 1 < \"a\"\n"),
    ("compare_ge", "let x = \"a\" ≥ 1\n"),
    ("negate", "let x = 0 - 1\nlet y = -\"a\"\n"),
    ("not", "let x = not 1\n"),
    ("and", "let x = true and 1\n"),
    ("or", "let x = false or 1\n"),
    ("if_condition", "if 1 {\n    let a = 1\n}\n"),
    ("assert_type", "assert 1\n"),
    ("assert_failed", "assert 1 = 2\n"),
    ("loop_operand", "loop x over 5 {\n    assert true\n}\n"),
    ("field_on_non_object", "let a = 1\nlet b = a.name\n"),
    // Nested failures, where the location is the *enclosing* top-level
    // statement rather than the line that failed — the accepted imprecision
    // of the top-level-only design, and worth pinning because both backends
    // have to be imprecise in the same place.
    (
        "nested_in_loop",
        "let xs = [1, 2]\nloop x over xs {\n    assert x = 1\n}\n",
    ),
    (
        "nested_in_if",
        "let a = 1\nif a = 1 {\n    assert a = 2\n}\n",
    ),
    ("index_non_container", "let a = 1\nlet b = a[0]\n"),
    // `Length`'s operand message is not here for the same reason
    // `code_check_particle`'s is not: since core answers with an Exception
    // rather than unwinding its caller, it no longer fails at the top level.
    // `emit_length_bad_operand_is_exception.code` asserts it in full under
    // both backends instead.
    // `code_check_particle`'s message is the one failure that *cannot* be
    // provoked at the top level: a handler returning a non-particle is a
    // failure of that frame, so since phase 4 it comes back as an Exception
    // and the program carries on. It is pinned instead by
    // `handler_return_non_particle_is_exception.code`, which asserts the
    // message in full and runs under both backends — the same guarantee this
    // file gives, reached the other way round.
];

/// All of it, location block included.
///
/// This compared only the first line until 2026-08-28, because `code run`
/// followed the message with a `--> file:line:col` block the compiled
/// backend had no equivalent for. It has one now
/// (`docs/todo/runtime-error-locations.md`), so the whole report is
/// compared — which also makes this the test that the locations agree, line
/// and column and caret alike, not just that they both exist.
fn report_of(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr).trim_end().to_string()
}

fn interpreted(name: &str, path: &Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("run")
        .arg(path)
        .output()
        .expect("run `code run` subprocess");
    assert!(
        !output.status.success(),
        "{name}: expected this program to fail under `code run`, but it ran. Every case \
         here must fail at the *top level*: a failure inside a handler is an Exception \
         value now, and the program carries on."
    );
    report_of(&output.stderr)
}

fn compiled(name: &str, path: &Path, exe: &Path) -> String {
    code::compile_file(path, code::BuildTarget::Exe, exe, false)
        .unwrap_or_else(|e| panic!("{name}: expected this to compile — the failure is meant to be a runtime one, not a compile-time one: {e}"));
    let output = Command::new(exe).output().expect("run compiled binary");
    assert!(
        !output.status.success(),
        "{name}: expected the compiled binary to fail, but it exited 0"
    );
    report_of(&output.stderr)
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|l| format!("      {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn both_backends_word_every_runtime_failure_identically() {
    let dir: PathBuf = std::env::temp_dir().join(format!("code-msg-parity-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test directory");

    let mut mismatches = Vec::new();
    for (name, src) in CASES {
        let path = dir.join(format!("{name}.code"));
        std::fs::write(&path, src).expect("write case");
        let run = interpreted(name, &path);
        let build = compiled(name, &path, &dir.join(name));
        if run != build {
            mismatches.push(format!(
                "  {name}\n    run:\n{}\n    build:\n{}",
                indent(&run),
                indent(&build)
            ));
        }
    }

    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        mismatches.is_empty(),
        "the two backends word {} failure(s) differently. `Exception.message` is a \
         value a program can read, so this is a behavioural difference, not a cosmetic \
         one:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

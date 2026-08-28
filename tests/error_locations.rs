//! End-to-end guard for error locations: lex and parse errors, wired up in
//! `loader.rs`, and runtime ones, wired up in `interpreter::run_with`.
//! `span.rs`'s own unit tests cover the *rendering* in isolation; these
//! cover that a real run actually goes through it — without them, dropping
//! either `map_err` would silently take error locations away and no test
//! would notice.

/// Runs `src` with no module story (`link` is refused) — the same entry
/// point the wasm playground uses, chosen here because it needs no file on
/// disk and no LLVM.
fn error_from(src: &str) -> String {
    code::run_source(src).expect_err("expected this program to fail")
}

#[test]
fn a_parse_error_carries_line_column_and_a_caret() {
    let err = error_from("let a = 1\nlet b = a +\n");
    assert!(err.contains(":2:"), "expected a line-2 location in:\n{err}");
    assert!(
        err.contains("2 | let b = a +"),
        "expected the source line in:\n{err}"
    );
    assert!(err.contains('^'), "expected a caret in:\n{err}");
}

#[test]
fn a_lex_error_carries_a_location_too() {
    let err = error_from("let a = 1\nlet b = @\n");
    assert!(err.contains("unexpected character '@'"), "{err}");
    assert!(
        err.contains(":2:9"),
        "expected the column of `@` in:\n{err}"
    );
}

/// `assert` is the error this whole feature exists for: it is the one
/// runtime message that names nothing at all, and the fixture suite is
/// mostly asserts.
#[test]
fn a_failing_assert_points_at_its_own_line() {
    let err = error_from("let a = 1\nassert a = 2\n");
    assert_eq!(
        err,
        "assertion failed\n --> <source>:2:1\n  |\n2 | assert a = 2\n  | ^"
    );
}

/// Every other runtime error goes through the same one place, so naming the
/// thing that went wrong and pointing at it are not alternatives.
///
/// A type mismatch rather than the undefined variable this used to use:
/// since 2026-08-28 `verify::verify_defined` runs before the interpreter
/// starts (see `interpreter::run_with`), so an undefined name never reaches
/// the runtime at all. Operand types still can't be known until the program
/// runs, so this one stays genuinely runtime.
#[test]
fn other_runtime_errors_are_located_too() {
    let err = error_from("let a = 1\nlet b = a + \"x\"\n");
    assert!(err.contains("cannot apply"), "{err}");
    assert!(
        err.contains(":2:1"),
        "expected a line-2 location in:\n{err}"
    );
}

/// The undefined name that used to be the case above: now refused before a
/// single statement runs, which is what `code build` has always done. Pinned
/// because it is a deliberate change of *when* a program fails, not an
/// accident — and because it is what keeps the two output modes agreeing
/// about which programs fail once a handler body's errors become values.
#[test]
fn an_undefined_name_is_refused_before_the_program_starts() {
    let err = error_from("emit Noisy {} to this\nlet a = q\n");
    assert!(err.contains("undefined variable 'q'"), "{err}");
}

/// The accepted imprecision of the top-level-only design: a failure inside
/// an `if` or `loop` body is reported against the top-level statement that
/// contains it, because that is the only statement the interpreter's
/// top-level loop knows an offset for. Pinned rather than merely noted — if
/// this ever becomes exact (spans on every `Stmt`), it should be because
/// someone decided to pay for it, not by accident.
#[test]
fn a_nested_failure_reports_the_enclosing_top_level_statement() {
    let err = error_from("let xs = [1, 2, 3]\nloop x over xs {\n  assert x < 3\n}\n");
    assert!(
        err.contains(":2:1") && err.contains("2 | loop x over xs {"),
        "expected the enclosing `loop` on line 2, not the inner assert on line 3:\n{err}"
    );
}

/// A program with no `origin` — one built by hand rather than parsed, as
/// `tests/module_host.rs` does — still errors exactly as it did before.
/// This is the path that keeps `Program::starts` optional rather than
/// load-bearing.
#[test]
fn a_program_without_an_origin_reports_a_bare_message() {
    use code::ast::{Expr, Program, Stmt};

    let program = Program {
        statements: vec![Stmt::Assert(Expr::Bool(false))],
        ..Default::default()
    };
    let err = code::interpreter::run(&program).expect_err("expected this program to fail");
    assert_eq!(err, "assertion failed");
}

/// The same enclosing-statement rule, one level up: a linked module's
/// statements are folded into a `Stmt::Import` body by the loader, so they
/// are no longer top-level and the failure is reported against the entry
/// file's `link` line. Strictly more than the bare message it replaced, but
/// worth pinning so nobody reads the caret as "the bug is on this line".
#[test]
fn a_failure_inside_a_linked_module_reports_the_link_line() {
    use std::fs;

    let dir = std::env::temp_dir().join(format!("code-error-loc-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    fs::write(dir.join("m.code"), "export let k = 1\nassert k = 2\n").expect("write module");
    let entry = dir.join("entry.code");
    fs::write(&entry, "let before = 1\nlink \"m.code\" as m\n").expect("write entry");

    let err = code::run_file(&entry).expect_err("expected this program to fail");
    assert!(
        err.contains("assertion failed") && err.contains("entry.code:2:1"),
        "expected the entry file's link line, got:\n{err}"
    );
    let _ = fs::remove_dir_all(&dir);
}

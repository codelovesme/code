//! End-to-end guard for the lex/parse error locations wired up in
//! `loader.rs`. `span.rs`'s own unit tests cover the *rendering* in
//! isolation; these cover that a real run actually goes through it — without
//! them, dropping the `map_err` in `load_source` would silently take error
//! locations away and no test would notice.

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

/// A runtime error still has no location — the deliberate scope line drawn
/// in `docs/todo/runtime-error-locations.md`. Asserted so that closing that
/// gap has to come here and update this test on purpose, rather than this
/// file quietly claiming coverage it doesn't have.
#[test]
fn runtime_errors_are_not_located_yet() {
    let err = error_from("let a = 1\nassert a = 2\n");
    assert_eq!(err, "assertion failed");
}

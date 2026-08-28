//! `code format` is checkable rather than trustworthy, and this is the check.
//!
//! The formatter only ever moves whitespace *between* tokens it sliced out of
//! the source — it never re-encodes a literal, never re-wraps a comment, and
//! has no opinion about semantics. That is worth something only if it is
//! proven, so three properties are asserted over every `tests/*.code` file,
//! which is the same "the fixtures are the tests" shape
//! `run_language_tests.rs` uses:
//!
//! - **Token equality.** The parser sees the identical token sequence before
//!   and after, so a formatted file cannot mean anything different. (`;` to
//!   newline is invisible here on purpose: both lex to `Newline`.)
//! - **Comment preservation.** Token equality says nothing at all about
//!   comments — the lexer drops them — so this is its necessary companion,
//!   and it is the property that would have caught an AST-based formatter
//!   silently eating every `--` block in this directory.
//! - **Idempotence.** `format(format(src)) == format(src)`.
//!
//! Deliberately written before the tree was reformatted, so the properties
//! were proven against the corpus as its authors had left it rather than
//! against the formatter's own output.

use std::path::{Path, PathBuf};

use code::lexer;

fn fixtures() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir("tests")
        .expect("read tests/")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "code"))
        .collect();
    paths.sort();
    assert!(
        paths.len() > 100,
        "expected the fixture corpus, found {paths:?}"
    );
    paths
}

/// Every `--` comment, in order.
///
/// Deliberately a second implementation of the gap walk rather than a call
/// into `format.rs`: a property checked with the very code it is checking
/// proves nothing. It leans on the same guarantee, though — the text between
/// two tokens holds only whitespace, `;`, and comments, because anything else
/// would have become a token, and a `--` inside a string literal is inside
/// that token.
fn comments(src: &str) -> Vec<String> {
    let lexed = lexer::tokenize(src).expect("fixture lexes");
    let chars: Vec<char> = src.chars().collect();
    let mut found = Vec::new();
    let mut gaps = vec![(0usize, lexed.starts.first().copied().unwrap_or(0) as usize)];
    for i in 0..lexed.tokens.len() {
        let start = lexed.ends[i] as usize;
        let end = lexed.starts.get(i + 1).copied().unwrap_or(0) as usize;
        gaps.push((start, end));
    }
    for (start, end) in gaps {
        let mut i = start;
        while i < end && i < chars.len() {
            if chars[i] == '-' && chars.get(i + 1) == Some(&'-') {
                let from = i;
                while i < end && i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                found.push(
                    chars[from..i]
                        .iter()
                        .collect::<String>()
                        .trim_end()
                        .to_string(),
                );
            } else {
                i += 1;
            }
        }
    }
    found
}

/// A fixture the formatter is *expected* to refuse: `fail_*.code` files that
/// are deliberate parse errors have no layout to canonicalize, and the point
/// of refusing them is that `--check` must not fail the CI gate over them.
fn formatted(path: &Path) -> Option<(String, String)> {
    let src = std::fs::read_to_string(path).expect("read fixture");
    let out = code::format::format(&src).ok()?;
    Some((src, out))
}

#[test]
fn formatting_never_changes_the_token_stream() {
    for path in fixtures() {
        let Some((src, out)) = formatted(&path) else {
            continue;
        };
        let before = lexer::tokenize(&src).expect("fixture lexes").tokens;
        let after = lexer::tokenize(&out)
            .unwrap_or_else(|e| {
                panic!(
                    "{}: formatted output does not lex: {}",
                    path.display(),
                    e.msg
                )
            })
            .tokens;
        assert_eq!(
            before,
            after,
            "{}: formatting changed the token stream",
            path.display()
        );
    }
}

#[test]
fn formatting_keeps_every_comment() {
    for path in fixtures() {
        let Some((src, out)) = formatted(&path) else {
            continue;
        };
        assert_eq!(
            comments(&src),
            comments(&out),
            "{}: formatting changed the comments",
            path.display()
        );
    }
}

#[test]
fn formatting_is_idempotent() {
    for path in fixtures() {
        let Some((_, out)) = formatted(&path) else {
            continue;
        };
        let again = code::format::format(&out).unwrap_or_else(|e| {
            panic!(
                "{}: formatted output does not re-format: {}",
                path.display(),
                e.msg
            )
        });
        assert_eq!(
            again,
            out,
            "{}: formatting is not idempotent",
            path.display()
        );
    }
}

/// The corpus is not all unformattable — if a change ever made `format`
/// refuse everything, the three properties above would pass vacuously.
#[test]
fn the_corpus_is_actually_being_formatted() {
    let total = fixtures().len();
    let formattable = fixtures().iter().filter(|p| formatted(p).is_some()).count();
    assert!(
        formattable > total * 3 / 4,
        "only {formattable} of {total} fixtures formatted; the rest should be \
         the deliberate `fail_*` parse errors alone"
    );
}

/// Every file this refuses must be a `fail_*.code`. A plain fixture that
/// stopped parsing would otherwise be quietly skipped by all three properties
/// above rather than reported.
#[test]
fn only_deliberate_parse_errors_are_refused() {
    for path in fixtures() {
        if formatted(&path).is_some() {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            name.starts_with("fail_"),
            "{name} does not parse, but only `fail_*` fixtures are allowed not to"
        );
    }
}

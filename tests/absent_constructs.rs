//! What this language answers when someone reaches for a construct it does
//! not have.
//!
//! The `fail_absent_*.code` fixtures prove those programs are refused. They
//! cannot prove *how*, and refusal was never the problem: `fn average(xs)`
//! was already refused, with a message about a missing `=` that sent the
//! reader looking for a typo in an assignment they had not written. So the
//! wording is the thing under test here, and it is only meaningful as a
//! whole-run check — delete the `absent_construct` arm and every fixture
//! still passes.
//!
//! See `docs/todo/errors-for-absent-constructs.md`.

/// Runs `src` with no module story (`link` is refused) — the same entry
/// point `tests/error_locations.rs` uses, for the same reasons: no file on
/// disk, no LLVM.
fn error_from(src: &str) -> String {
    code::run_source(src).expect_err("expected this program to fail")
}

/// Every spelling of "define a function" answers by saying there are none,
/// and shows the shape that replaces them.
#[test]
fn reaching_for_a_function_is_answered_with_handlers() {
    for src in [
        "fn average(xs) {\n}\n",
        "def average(xs) {\n}\n",
        "func average(xs) {\n}\n",
        "fun average(xs) {\n}\n",
        "function average(xs) {\n}\n",
        "lambda average(xs) {\n}\n",
    ] {
        let err = error_from(src);
        assert!(
            err.contains("there are no functions"),
            "expected the functions message for {src:?}, got:\n{err}"
        );
        assert!(
            err.contains("=>") && err.contains("emit"),
            "the message should show the handler shape and how to reach it; got:\n{err}"
        );
    }
}

#[test]
fn reaching_for_a_loop_keyword_names_the_loop_that_exists() {
    let err = error_from("let i = 0\nwhile i < 3 {\n    i = i + 1\n}\n");
    assert!(err.contains("there is no `while`"), "{err}");
    assert!(
        err.contains("loop { }"),
        "should name the shape; got:\n{err}"
    );

    for src in ["for x in [1, 2] {\n}\n", "foreach x in [1, 2] {\n}\n"] {
        let err = error_from(src);
        assert!(err.contains("there is no `for`"), "{err}");
        assert!(
            err.contains("loop item over container"),
            "should name the shape; got:\n{err}"
        );
    }
}

/// `else` is the one that does not start a statement — the `}` it follows
/// has already closed the `if` body, so it is answered from
/// `expect_end_of_statement` rather than from the bare-identifier arm.
#[test]
fn else_is_answered_where_it_actually_lands() {
    let err = error_from("if true {\n    let a = 1\n} else {\n    let a = 2\n}\n");
    assert!(err.contains("there is no `else`"), "{err}");
    assert!(
        err.contains("second `if`"),
        "should say what to write instead; got:\n{err}"
    );
}

#[test]
fn print_import_and_type_declarations_are_answered_too() {
    let err = error_from("print \"hello\"\n");
    assert!(err.contains("there is no print statement"), "{err}");
    assert!(
        err.contains("console"),
        "should point at the module; got:\n{err}"
    );

    let err = error_from("import \"foo\"\n");
    assert!(err.contains("there is no `import`"), "{err}");
    assert!(err.contains("link"), "should name `link`; got:\n{err}");

    let err = error_from("class Point {\n}\n");
    assert!(err.contains("there are no type declarations"), "{err}");
}

/// The message points at the word that was typed, not at whatever followed
/// it. `advance` moves `err_pos` past the name before the `=` is demanded,
/// so this is the one thing about these errors that is easy to regress
/// without noticing.
#[test]
fn the_caret_points_at_the_keyword() {
    let err = error_from("fn average(xs) {\n}\n");
    assert!(err.contains(":1:1"), "expected column 1 in:\n{err}");
    assert!(
        err.contains("1 | fn average(xs) {"),
        "expected the source line in:\n{err}"
    );
}

/// None of these words is reserved. They are ordinary identifiers, and the
/// hint fires only where an assignment was expected and did not arrive — so
/// a program that genuinely uses one as a variable is untouched.
#[test]
fn the_words_are_still_ordinary_identifiers() {
    code::run_source("let print = 1\nassert print = 1\n").expect("`print` as a name");
    code::run_source("let print = 1\nprint = 2\nassert print = 2\n").expect("reassignment");
    code::run_source("let print = 1\nprint += 1\nassert print = 2\n").expect("compound");
    code::run_source("let for = 1\nfor = 5\nassert for = 5\n").expect("`for` as a name");
    code::run_source("let type = \"x\"\nassert type = \"x\"\n").expect("`type` as a name");
}

/// The marker that changed. Anyone with a file written before 1.4.0 meets
/// this first, and the generic answer — `unexpected character '`'`, from
/// somewhere in the middle of the prose — says nothing about what happened.
#[test]
fn an_old_comment_marker_names_the_new_one() {
    // Deliberately full of what real comments contain: backticks and a
    // version number, both of which fail to lex. The message has to come
    // from the marker, not from the text after it.
    let err = error_from("-- a comment with `backticks` and 1.4.0 in it\nassert 1 = 1\n");
    assert!(err.contains("`--` is not a comment"), "{err}");
    assert!(err.contains("`|`"), "should name the marker; got:\n{err}");
    assert!(
        err.contains(":1:1"),
        "should point at the marker; got:\n{err}"
    );

    // Indented too — a comment inside a block is still a comment.
    let err = error_from("if true {\n    -- indented\n    assert 1 = 1\n}\n");
    assert!(err.contains("`--` is not a comment"), "{err}");
}

/// …and nowhere else. `--` stopped being special when the marker moved, and
/// `5--1` is `5 - -1` — the very ambiguity the old marker created. Refusing
/// `--` outright, the way `!` and `;` are refused, would take that with it.
#[test]
fn double_minus_is_still_arithmetic_anywhere_but_a_line_start() {
    code::run_source("let n = 5--1\nassert n = 6\n").expect("5--1 is 5 - -1");
    code::run_source("let a = 3\nlet b = 1\nlet m = a - -b\nassert m = 4\n").expect("a - -b");
    code::run_source("let xs = [1, --2]\nassert xs = [1, 2]\n").expect("--2 inside a literal");
}

//! Source locations for error messages.
//!
//! Deliberately confined to one boundary: `Located` is produced by `lexer`
//! and `parser`, and consumed by `loader`, which is the only place holding
//! both a module's source text and its name. Everything downstream — the
//! interpreter, codegen, `runtime.c` — keeps plain `String` errors and never
//! learns that offsets exist, so this whole feature is three files wide and
//! can be changed or removed without touching the language itself.
//!
//! That containment is also what bounds it: locating a *runtime* error would
//! mean carrying spans on AST nodes, a much more invasive change kept
//! deliberately out of scope — see `docs/todo/runtime-error-locations.md`.

/// An error that may know where in its source it happened.
///
/// `at` is a **char** offset, not a byte one: the lexer indexes source as
/// `Vec<char>`, so a multi-byte operator (`≠`, `≤`, `≥`) earlier on the line
/// doesn't skew the column that `render` reports.
#[derive(Debug, Clone)]
pub struct Located {
    pub at: Option<u32>,
    pub msg: String,
}

impl Located {
    pub fn at(offset: usize, msg: impl Into<String>) -> Self {
        Located {
            at: Some(offset as u32),
            msg: msg.into(),
        }
    }
}

/// Lets `?` lift a plain `String` error into a position-less `Located`, so a
/// site with nothing useful to point at doesn't have to say so explicitly.
impl From<String> for Located {
    fn from(msg: String) -> Self {
        Located { at: None, msg }
    }
}

/// Renders a message as a rustc-style block pointing into `source`:
///
/// ```text
/// expected an expression, found Star
///  --> demo.code:2:9
///   |
/// 2 | let b = *
///   |         ^
/// ```
///
/// The caller prefixes `error: ` (see `main.rs`), matching how a bare
/// message is printed today. With no offset, the message is returned
/// unchanged — an unlocated error still reads exactly as it did before.
pub fn render(source: &str, file: &str, at: Option<u32>, msg: &str) -> String {
    let Some(at) = at else {
        return msg.to_string();
    };

    let chars: Vec<char> = source.chars().collect();
    // Clamped rather than trusted: an offset one past the end is normal (the
    // synthetic `Eof` token points there), and it should render as the last
    // column rather than panic.
    let at = (at as usize).min(chars.len());

    let mut line_start = 0;
    let mut line_no = 1;
    for (i, &c) in chars.iter().enumerate() {
        if i >= at {
            break;
        }
        if c == '\n' {
            line_start = i + 1;
            line_no += 1;
        }
    }
    let line_end = chars[line_start..]
        .iter()
        .position(|&c| c == '\n')
        .map_or(chars.len(), |p| line_start + p);

    let line: String = chars[line_start..line_end].iter().collect();
    let col = at - line_start + 1;

    // The gutter is as wide as the line number, so the `|` rules line up
    // whether the error is on line 3 or line 1042.
    let num = line_no.to_string();
    let pad = " ".repeat(num.len());
    let caret = " ".repeat(col - 1);

    format!("{msg}\n{pad}--> {file}:{line_no}:{col}\n{pad} |\n{num} | {line}\n{pad} | {caret}^")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_at_the_offending_column() {
        let src = "let a = 1\nlet b = *\n";
        // The `*` is at char offset 18.
        let out = render(src, "demo.code", Some(18), "expected an expression");
        assert_eq!(
            out,
            "expected an expression\n --> demo.code:2:9\n  |\n2 | let b = *\n  |         ^"
        );
    }

    /// A multi-byte operator before the error must not push the caret out of
    /// alignment — the whole reason offsets are counted in chars.
    #[test]
    fn multibyte_operators_do_not_skew_the_column() {
        let src = "assert 1 ≠ 2 *\n";
        let star = src.chars().position(|c| c == '*').unwrap() as u32;
        let out = render(src, "d.code", Some(star), "boom");
        assert!(out.contains(":1:14"), "{out}");
        // The caret lands under the `*` in the echoed source line — compared
        // by position in the rendered output rather than a hand-counted
        // string, so this stays honest if the gutter format changes.
        let lines: Vec<&str> = out.lines().collect();
        let (source_line, caret_line) = (lines[lines.len() - 2], lines[lines.len() - 1]);
        assert_eq!(
            caret_line.find('^'),
            source_line.chars().position(|c| c == '*'),
            "caret under `*`\n{out}"
        );
    }

    #[test]
    fn no_offset_leaves_the_message_alone() {
        assert_eq!(render("x", "f.code", None, "plain"), "plain");
    }
}

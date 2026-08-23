//! Turning a `code::span::Located` parse/lex error into an LSP `Diagnostic`.
//!
//! Deliberately does not run the interpreter: the only static errors this
//! language has are lex and parse errors (it is dynamically typed, and
//! every type/undefined-name check happens at `run` time — see
//! `docs/todo/runtime-error-locations.md`). Executing a program just
//! because the user is editing it would mean side effects (a linked
//! module's `Print`, an infinite `loop {}`) firing on every keystroke, so
//! that's out of scope on purpose, not an oversight.

use code::span::Located;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

/// `at` is a **char** offset (see `Located`'s doc comment); LSP `Position`
/// columns are UTF-16 code units, so this walks `src` counting UTF-16 units
/// per char rather than assuming the two coincide.
fn offset_to_position(chars: &[char], at: usize) -> Position {
    let at = at.min(chars.len());
    let (mut line, mut col) = (0u32, 0u32);
    for &c in &chars[..at] {
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += c.len_utf16() as u32;
        }
    }
    Position::new(line, col)
}

/// One `Located` error, or `None` for an empty document with nothing to
/// report — every call site clears diagnostics by publishing an empty list
/// in that case, same as a document that now parses cleanly.
pub fn from_located(text: &str, err: &Located) -> Diagnostic {
    let chars: Vec<char> = text.chars().collect();
    let start = err.at.map(|at| at as usize).unwrap_or(0);
    let range = Range::new(
        offset_to_position(&chars, start),
        // A one-char range: nothing upstream hands us how wide the
        // offending token was (`Located` carries a single offset, not a
        // span — seconding that as an LSP squiggle needs at least one
        // character to actually be visible).
        offset_to_position(&chars, start + 1),
    );
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("code-lsp".into()),
        message: err.msg.clone(),
        ..Default::default()
    }
}

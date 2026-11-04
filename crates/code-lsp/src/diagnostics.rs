//! Map shared non-executing checker results into UTF-16 LSP diagnostics.

use code::ast::Span;
use code::span::Located;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

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

pub fn message(text: &str, span: Option<Span>, code: &str, message: &str) -> Diagnostic {
    let chars: Vec<char> = text.chars().collect();
    let span = span.unwrap_or(Span::new(0, 1));
    Diagnostic {
        range: Range::new(
            offset_to_position(&chars, span.start as usize),
            offset_to_position(&chars, span.end as usize),
        ),
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("code-lsp".into()),
        code: Some(NumberOrString::String(code.to_string())),
        message: message.to_string(),
        ..Default::default()
    }
}

pub fn from_static(diagnostic: &code::diagnostics::Diagnostic, text: &str) -> Diagnostic {
    let mut result = message(text, diagnostic.span, diagnostic.code, &diagnostic.message);
    result.severity = Some(match diagnostic.severity {
        code::diagnostics::Severity::Error => DiagnosticSeverity::ERROR,
        code::diagnostics::Severity::Warning => DiagnosticSeverity::WARNING,
    });
    result
}

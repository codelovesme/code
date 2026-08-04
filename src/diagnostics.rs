//! Human-friendly rendering of source diagnostics (currently parse errors).
//!
//! Produces a rustc-style block pointing at the offending source with a caret:
//!
//! ```text
//! error: unexpected token
//!  --> hello.code:2:5
//!   |
//! 2 | y = @
//!   |     ^
//! ```

/// Render one diagnostic. `start`/`end` are **char** offsets into `source`
/// (matching `chumsky`'s span indices, so multi-byte operators like `≤`/`∈`
/// don't skew the column).
pub fn render(source: &str, file: &str, start: usize, end: usize, message: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let start = start.min(chars.len());
    let end = end.clamp(start, chars.len());

    // Find the start-of-line char index and 1-based line number for `start`.
    let mut line_start = 0usize;
    let mut line_no = 1usize;
    for (i, &c) in chars.iter().enumerate() {
        if i >= start {
            break;
        }
        if c == '\n' {
            line_start = i + 1;
            line_no += 1;
        }
    }
    let col = start - line_start; // 0-based column

    // The offending source line (without its trailing newline).
    let line_end = chars[line_start..]
        .iter()
        .position(|&c| c == '\n')
        .map(|off| line_start + off)
        .unwrap_or(chars.len());
    let line_text: String = chars[line_start..line_end].iter().collect();

    // Caret underlines the span, clamped to this line; at least one `^`.
    let caret_len = end.min(line_end).saturating_sub(start).max(1);

    let gutter = line_no.to_string();
    let pad = " ".repeat(gutter.len());

    format!(
        "error: {message}\n\
         {pad} --> {file}:{line}:{col}\n\
         {pad} |\n\
         {gutter} | {line_text}\n\
         {pad} | {caret_indent}{caret}",
        line = line_no,
        col = col + 1,
        caret_indent = " ".repeat(col),
        caret = "^".repeat(caret_len),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_caret_at_column() {
        let src = "x = 1\ny = @\nz = 2\n";
        // '@' is char offset 10 → line 2, column 5.
        let out = render(src, "f.code", 10, 11, "unexpected token");
        assert!(out.contains("error: unexpected token"), "{out}");
        assert!(out.contains("--> f.code:2:5"), "{out}");
        assert!(out.contains("2 | y = @"), "{out}");
        assert!(out.contains("  |     ^"), "{out}");
    }

    #[test]
    fn column_is_char_based_not_byte_based() {
        // `≤` is 3 bytes but 1 char; a byte-based column would be wrong here.
        let src = "a ≤ 3\nb = @\n";
        let out = render(src, "f", 10, 11, "bad");
        assert!(out.contains("--> f:2:5"), "{out}");
    }

    #[test]
    fn multi_char_span_widens_the_caret() {
        let src = "value = xyz\n";
        // "xyz" spans chars 8..11.
        let out = render(src, "f", 8, 11, "unknown");
        assert!(out.contains("        ^^^"), "{out}");
    }

    #[test]
    fn handles_offset_at_end_of_input() {
        let src = "a = 1\n";
        let out = render(src, "f", src.chars().count(), src.chars().count(), "unexpected end");
        assert!(out.contains("--> f:"), "{out}");
        assert!(out.contains('^'), "{out}");
    }
}

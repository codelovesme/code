//! Source formatter for `.code` files.
//!
//! A lightweight, brace-driven re-indenter: it trims each line and re-applies
//! indentation from `{`/`}` nesting depth, ignoring braces inside strings and
//! `->` comments. Shared by the `code format` CLI command and the language server
//! so both produce identical output.

/// Re-indent `text` using `indent_size` spaces per nesting level.
///
/// Blank lines are preserved (emptied of trailing whitespace); the result always
/// ends with a single trailing newline.
pub fn format_document(text: &str, indent_size: usize) -> String {
    let indent_str = " ".repeat(indent_size);
    let mut result: Vec<String> = Vec::new();
    let mut depth: usize = 0;

    for line in text.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            result.push(String::new());
            continue;
        }

        // Leading `}` dedent before the line is written.
        let leading_closes = trimmed.chars().take_while(|c| *c == '}').count();

        let effective_depth = depth.saturating_sub(leading_closes);
        let indented = format!("{}{}", indent_str.repeat(effective_depth), trimmed);
        result.push(indented);

        // Update depth from braces (ignoring those in strings / comments).
        let (opens, closes) = count_braces(trimmed);
        depth = (depth + opens).saturating_sub(closes);
    }

    let mut formatted = result.join("\n");
    if !formatted.ends_with('\n') {
        formatted.push('\n');
    }
    formatted
}

/// Count `{` and `}` on a line, skipping any inside double-quoted strings or
/// after a `->` line comment.
fn count_braces(line: &str) -> (usize, usize) {
    let chars: Vec<char> = line.chars().collect();
    let mut opens = 0usize;
    let mut closes = 0usize;
    let mut in_string = false;
    let mut i = 0;

    while i < chars.len() {
        if in_string {
            if chars[i] == '\\' {
                i += 2;
                continue;
            }
            if chars[i] == '"' {
                in_string = false;
            }
        } else {
            // Comment start — rest of line is a comment.
            if chars[i] == '-' && i + 1 < chars.len() && chars[i + 1] == '>' {
                break;
            }
            match chars[i] {
                '"' => in_string = true,
                '{' => opens += 1,
                '}' => closes += 1,
                _ => {}
            }
        }
        i += 1;
    }

    (opens, closes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reindents_nested_blocks() {
        let input = "loop {\nif x {\nyield x\n}\n}\n";
        let expected = "loop {\n    if x {\n        yield x\n    }\n}\n";
        assert_eq!(format_document(input, 4), expected);
    }

    #[test]
    fn strips_trailing_and_normalizes_indent() {
        let input = "a = 1   \n      b = 2\n";
        assert_eq!(format_document(input, 4), "a = 1\nb = 2\n");
    }

    #[test]
    fn ignores_braces_in_strings_and_comments() {
        let input = "msg = \"a { b } c\"\nx = 1  -> trailing } brace\n";
        // No real nesting: nothing should be indented.
        assert_eq!(
            format_document(input, 4),
            "msg = \"a { b } c\"\nx = 1  -> trailing } brace\n"
        );
    }

    #[test]
    fn already_formatted_is_idempotent() {
        let src = "loop {\n    if x {\n        yield x\n    }\n}\n";
        assert_eq!(format_document(src, 4), src);
    }

    #[test]
    fn ensures_trailing_newline() {
        assert_eq!(format_document("a = 1", 4), "a = 1\n");
    }
}

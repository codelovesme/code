//! One-off: rewrites a file's brace blocks as indented ones.
//!
//! Temporary, and it deletes itself once both repositories have run through
//! it. It exists at all because a block's braces and an object's are the same
//! character in the same shape, so no token walk can tell them apart — but
//! the parser has already decided which is which by the time it consumes
//! them, and `parser::parse_recording_block_braces` hands those offsets over
//! rather than making this guess. That is AGENTS.md's "never `sed` a syntax
//! migration" rule, applied to braces.
//!
//! Run `code format` first: the bodies keep whatever indentation they have,
//! and canonical layout is what makes them already correct once the braces
//! are gone.

use crate::lexer;
use crate::parser;
use crate::span::Located;

/// The file with every block's braces removed, and one-line bodies rejoined
/// to their header with `,` (or with nothing, after a `=>`).
///
/// Object literals are untouched — they are what braces mean from now on.
pub fn to_indentation(src: &str) -> Result<String, Located> {
    let lexed = lexer::tokenize(src)?;
    let (_, blocks) = parser::parse_recording_block_braces(&lexed)?;
    let chars: Vec<char> = src.chars().collect();

    // (start, end, replacement) over char indices, applied back to front so
    // earlier offsets stay valid.
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for block in blocks {
        let open = block.open as usize;
        let close = block.close as usize;

        // A block whose `{` is the last thing on its line was already written
        // across lines, so its body is already indented and only the braces
        // have to go. Anything else is a one-liner and has to be rejoined.
        let multiline = chars[open + 1..]
            .iter()
            .take_while(|&&c| c != '\n')
            .all(|c| c.is_whitespace());

        let mut before = open;
        while before > 0 && (chars[before - 1] == ' ' || chars[before - 1] == '\t') {
            before -= 1;
        }

        if multiline {
            edits.push((before, open + 1, String::new()));
            // The closing brace stands alone on its line, so the line goes
            // with it. If something shares the line, take only the brace and
            // leave the rest for a person to look at.
            let line_start = chars[..close]
                .iter()
                .rposition(|&c| c == '\n')
                .map(|n| n + 1)
                .unwrap_or(0);
            let line_end = close
                + 1
                + chars[close + 1..]
                    .iter()
                    .take_while(|&&c| c != '\n')
                    .count();
            let alone = chars[line_start..close].iter().all(|c| c.is_whitespace())
                && chars[close + 1..line_end].iter().all(|c| c.is_whitespace());
            if alone {
                // Swallow the newline too, or an empty line is left behind.
                let end = (line_end + 1).min(chars.len());
                edits.push((line_start, end, String::new()));
            } else {
                edits.push((close, close + 1, String::new()));
            }
        } else {
            let joiner = if block.after_arrow { " " } else { ", " };
            edits.push((before, open + 1, joiner.to_string()));
            let mut trailing = close;
            while trailing > 0 && (chars[trailing - 1] == ' ' || chars[trailing - 1] == '\t') {
                trailing -= 1;
            }
            edits.push((trailing, close + 1, String::new()));
        }
    }

    edits.sort_by_key(|(start, _, _)| *start);
    let mut out: Vec<char> = chars.clone();
    for (start, end, text) in edits.into_iter().rev() {
        out.splice(start..end, text.chars());
    }
    Ok(out.into_iter().collect())
}

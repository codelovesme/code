//! Semantic token classification for `.code` source.
//!
//! Built on the real lexer (`code::lexer::tokenize`) rather than a second,
//! hand-rolled scanner — `Lexed::ends` gives every token its exact source
//! span, so classifying is just walking `Lexed` and labeling each token by
//! kind. The lexer strips comments outright (they produce no token at all),
//! so those are recovered separately by scanning the gaps `Lexed` leaves
//! between tokens for a `--` run — the same gap a `.code` formatter would
//! need to preserve them (see `docs/todo/formatter.md`'s `Lexed::ends`
//! section, which this crate's addition of that field also serves).
//!
//! Positions are UTF-16 code units, matching the LSP spec (`Position` is
//! UTF-16-based) — a naive char count would misalign highlighting after any
//! non-BMP character in a string literal.

use code::lexer::{tokenize, Token};

/// Semantic token kinds this classifier emits. Order is the wire-protocol
/// legend index — do not reorder without updating `LEGEND_TYPES` alongside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Comment,
    String,
    Number,
    Keyword,
    /// An uppercase-first identifier — a particle class name
    /// (`Length`, `Timestamp`, a user-defined `Log`), the one naming
    /// convention this language's grammar itself leans on (see
    /// `parser.rs`'s particle-construction arm).
    Class,
    /// An identifier immediately after `.` — `expr.field`.
    Property,
    Variable,
    Operator,
}

/// The legend the LSP client is told about in `ServerCapabilities`, in the
/// same order as `Kind`'s discriminants.
pub const LEGEND_TYPES: &[&str] = &[
    "comment", "string", "number", "keyword", "class", "property", "variable", "operator",
];

/// A classified token: 0-based line/column (UTF-16 units) and length
/// (UTF-16 units), ready for LSP delta-encoding.
pub struct SemToken {
    pub line: u32,
    pub col: u32,
    pub len: u32,
    pub kind: u32,
}

/// `None` for a token kind that carries no useful color — brackets, `,`,
/// `:`, `.` itself, and the statement-separator `Newline`/`Eof` markers.
/// `prev_dot` is whether the immediately preceding real token was `.`,
/// which is what turns an identifier into a `Property` instead of a
/// `Variable`/`Class`.
fn classify(tok: &Token, prev_dot: bool) -> Option<Kind> {
    use Token::*;
    Some(match tok {
        Str(_) => Kind::String,
        // Colors as one string, interpolations included. The token stream
        // carries a single span per token, so picking the `$name` runs out
        // as variables would mean reshaping it — not worth it for the tint.
        InterpStr(_) => Kind::String,
        Number(_) => Kind::Number,
        True | False | Null | And | Or | Not | Assert | If | Let | Loop | Over | Break
        | Continue | Link | As | Export | Emit | To | Core | Get | Is | This | Return => {
            Kind::Keyword
        }
        Equals | Plus | PlusEq | Minus | Star | Slash | NotEq | Lt | Gt | Le | Ge | Arrow | In => {
            Kind::Operator
        }
        Ident(name) if prev_dot => {
            let _ = name;
            Kind::Property
        }
        Ident(name) if name.chars().next().is_some_and(char::is_uppercase) => Kind::Class,
        Ident(_) => Kind::Variable,
        LBracket | RBracket | LBrace | RBrace | LParen | RParen | Colon | Comma | Dot | Newline
        | Eof => return None,
    })
}

fn step(c: char, line: &mut u32, col: &mut u32) {
    if c == '\n' {
        *line += 1;
        *col = 0;
    } else {
        *col += c.len_utf16() as u32;
    }
}

fn step_range(chars: &[char], idx: &mut usize, line: &mut u32, col: &mut u32, end: usize) {
    while *idx < end {
        step(chars[*idx], line, col);
        *idx += 1;
    }
}

fn utf16_len(chars: &[char]) -> u32 {
    chars.iter().map(|c| c.len_utf16() as u32).sum()
}

/// Classify every token (plus recovered comments) in `src`, in document
/// order. Unlexable source (an unterminated string, a stray `!`) yields no
/// tokens at all rather than a partial or guessed classification — the
/// diagnostic already reported from `lex`/`parse` is what explains why
/// nothing lit up.
pub fn semantic_tokens(src: &str) -> Vec<SemToken> {
    let Ok(lexed) = tokenize(src) else {
        return Vec::new();
    };
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let (mut line, mut col, mut idx, mut ti) = (0u32, 0u32, 0usize, 0usize);
    let mut prev_dot = false;

    while idx < chars.len() {
        if ti < lexed.tokens.len() && lexed.starts[ti] as usize == idx {
            let tok = &lexed.tokens[ti];
            let end = lexed.ends[ti] as usize;
            if let Some(kind) = classify(tok, prev_dot) {
                out.push(SemToken {
                    line,
                    col,
                    len: utf16_len(&chars[idx..end]),
                    kind: kind as u32,
                });
            }
            prev_dot = matches!(tok, Token::Dot);
            step_range(&chars, &mut idx, &mut line, &mut col, end);
            ti += 1;
            continue;
        }

        // Not the start of a real token, so we're in a gap: either a `--`
        // comment or plain whitespace between tokens.
        if chars[idx] == '-' && chars.get(idx + 1) == Some(&'-') {
            let (start_line, start_col, start) = (line, col, idx);
            while idx < chars.len() && chars[idx] != '\n' {
                step(chars[idx], &mut line, &mut col);
                idx += 1;
            }
            out.push(SemToken {
                line: start_line,
                col: start_col,
                len: utf16_len(&chars[start..idx]),
                kind: Kind::Comment as u32,
            });
            prev_dot = false;
            continue;
        }

        step(chars[idx], &mut line, &mut col);
        idx += 1;
    }

    out
}

/// Delta-encode tokens per the LSP `semanticTokens/full` wire format: each
/// token becomes 5 integers (deltaLine, deltaStartChar, length, tokenType,
/// tokenModifiers). `tokens` must already be in document order.
pub fn encode_deltas(tokens: &[SemToken]) -> Vec<u32> {
    let mut data = Vec::with_capacity(tokens.len() * 5);
    let (mut prev_line, mut prev_col) = (0u32, 0u32);
    for t in tokens {
        let delta_line = t.line - prev_line;
        let delta_col = if delta_line == 0 {
            t.col - prev_col
        } else {
            t.col
        };
        data.extend([delta_line, delta_col, t.len, t.kind, 0]);
        prev_line = t.line;
        prev_col = t.col;
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every fixture below is one line of ASCII, so a token's UTF-16 column
    /// is also its char offset into `src` — this deliberately does not
    /// handle multi-line input or non-BMP characters.
    fn kinds(src: &str) -> Vec<(&'static str, String)> {
        let chars: Vec<char> = src.chars().collect();
        semantic_tokens(src)
            .into_iter()
            .map(|t| {
                assert_eq!(t.line, 0, "single-line fixture");
                let start = t.col as usize;
                let text: String = chars[start..start + t.len as usize].iter().collect();
                (LEGEND_TYPES[t.kind as usize], text)
            })
            .collect()
    }

    #[test]
    fn classifies_keyword_class_and_variable() {
        let got = kinds("if a { Point {} }");
        assert_eq!(got[0], ("keyword", "if".into()));
        assert_eq!(got[1], ("variable", "a".into()));
        assert_eq!(got[2], ("class", "Point".into()));
    }

    #[test]
    fn classifies_property_after_dot() {
        let got = kinds("let p = point.x");
        assert!(got.contains(&("variable", "point".into())));
        assert!(got.contains(&("property", "x".into())));
    }

    #[test]
    fn classifies_comment_and_string() {
        let got = kinds("-- a comment");
        assert_eq!(got[0].0, "comment");
        let got = kinds("let a = \"hello\"");
        assert!(got.iter().any(|(k, t)| *k == "string" && t == "\"hello\""));
    }

    #[test]
    fn classifies_number_and_literal_keywords() {
        let got = kinds("let a = 42 and true");
        assert!(got.iter().any(|(k, t)| *k == "number" && t == "42"));
        assert!(got.iter().any(|(k, t)| *k == "keyword" && t == "and"));
        assert!(got.iter().any(|(k, t)| *k == "keyword" && t == "true"));
    }

    #[test]
    fn classifies_unicode_operator_as_single_token() {
        let got = kinds("assert a ≤ 3");
        assert!(got.iter().any(|(k, t)| *k == "operator" && t == "≤"));
    }

    #[test]
    fn skips_unlexable_source_entirely() {
        assert!(semantic_tokens("let a = \"unterminated").is_empty());
    }

    #[test]
    fn delta_encoding_is_document_order_relative() {
        let toks = vec![
            SemToken {
                line: 0,
                col: 0,
                len: 2,
                kind: Kind::Keyword as u32,
            },
            SemToken {
                line: 0,
                col: 3,
                len: 1,
                kind: Kind::Variable as u32,
            },
            SemToken {
                line: 1,
                col: 0,
                len: 4,
                kind: Kind::Keyword as u32,
            },
        ];
        let data = encode_deltas(&toks);
        assert_eq!(
            data,
            vec![
                0,
                0,
                2,
                Kind::Keyword as u32,
                0,
                0,
                3,
                1,
                Kind::Variable as u32,
                0,
                1,
                0,
                4,
                Kind::Keyword as u32,
                0,
            ]
        );
    }
}

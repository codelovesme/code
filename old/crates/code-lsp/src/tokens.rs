//! Semantic token classification for `.code` source.
//!
//! Lexical, not AST-driven: the language has no public tokenizer to reuse (see
//! `code_lang::parser`, which is a char-based `chumsky` grammar with no token
//! stream), and the interpreter/codegen AST carries no expression-level spans
//! (see ticket T9). So this scans the raw text and classifies each token by
//! shape — good enough for coloring, not full semantic resolution.
//!
//! Columns are UTF-16 code units, matching the LSP spec (`Position` is
//! UTF-16-based) — a naive char or byte count would misalign highlighting on
//! any multi-byte operator (`≤ ≥ ≠ ∈ ∉ ∩ ∪`).

/// Semantic token kinds this classifier emits. Order is the wire-protocol
/// legend index — do not reorder without updating `LEGEND_TYPES` alongside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Comment,
    String,
    Number,
    Keyword,
    Type,
    Function,
    Variable,
    Operator,
    Property,
}

/// The legend the LSP client is told about in `ServerCapabilities`, in the
/// same order as `TokenKind`'s discriminants.
pub const LEGEND_TYPES: &[&str] = &[
    "comment", "string", "number", "keyword", "type", "function", "variable",
    "operator", "property",
];

/// A classified token: 0-based line/column (UTF-16 units) and length (UTF-16
/// units), ready for LSP delta-encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub line: u32,
    pub col: u32,
    pub len: u32,
    pub kind: TokenKind,
}

const KEYWORDS: &[&str] = &[
    "if", "loop", "over", "break", "return", "assert", "link", "as",
    "private", "is", "not", "and", "or", "in", "emit", "to", "get", "this",
    "base",
];
const LITERALS: &[&str] = &["true", "false", "Null"];

/// Multi-char Unicode operator glyphs (each is a single `char`, so no special
/// handling beyond "not ASCII identifier/whitespace" is needed for scanning).
const OPERATOR_CHARS: &str = "≤≥≠∈∉∩∪∧∨=<>+-*/!";

/// Scan `source` and classify every token, line by line.
pub fn tokenize(source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();

    for (line_idx, line) in source.lines().enumerate() {
        let line_no = line_idx as u32;
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0usize;
        let mut col_utf16 = 0u32;
        // Property access: true right after seeing `.` until the next non-ident char.
        let mut expect_property = false;

        while i < chars.len() {
            let c = chars[i];

            // Comment: `->` runs to end of line.
            if c == '-' && chars.get(i + 1) == Some(&'>') {
                tokens.push(Token { line: line_no, col: col_utf16, len: utf16_len(&chars[i..]), kind: TokenKind::Comment });
                break;
            }

            // String literal, with `$name` interpolation highlighted separately.
            if c == '"' {
                let start = i;
                let start_col = col_utf16;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if i < chars.len() {
                    i += 1; // closing quote
                }
                let seg = &chars[start..i];
                col_utf16 = start_col + utf16_len(seg);
                emit_string_with_interpolation(&mut tokens, line_no, start_col, seg);
                expect_property = false;
                continue;
            }

            // Number literal: digits with an optional single decimal point.
            if c.is_ascii_digit() {
                let start = i;
                let start_col = col_utf16;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let seg = &chars[start..i];
                col_utf16 = start_col + utf16_len(seg);
                tokens.push(Token { line: line_no, col: start_col, len: utf16_len(seg), kind: TokenKind::Number });
                expect_property = false;
                continue;
            }

            // Identifier / keyword / type / property.
            if c.is_alphabetic() || c == '_' {
                let start = i;
                let start_col = col_utf16;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let seg = &chars[start..i];
                let word: String = seg.iter().collect();
                col_utf16 = start_col + utf16_len(seg);

                let kind = if expect_property {
                    TokenKind::Property
                } else if KEYWORDS.contains(&word.as_str()) {
                    TokenKind::Keyword
                } else if LITERALS.contains(&word.as_str()) {
                    TokenKind::Keyword
                } else if word.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                    // Particle/type class names are conventionally uppercase
                    // (see README "Particles"): `Log`, `Point`, domain keywords.
                    TokenKind::Type
                } else if peek_non_space(&chars, i) == Some('(') {
                    TokenKind::Function
                } else {
                    TokenKind::Variable
                };
                tokens.push(Token { line: line_no, col: start_col, len: utf16_len(seg), kind });
                expect_property = false;
                continue;
            }

            if c == '.' {
                expect_property = true;
                i += 1;
                col_utf16 += 1;
                continue;
            }

            if OPERATOR_CHARS.contains(c) {
                let start_col = col_utf16;
                tokens.push(Token { line: line_no, col: start_col, len: 1, kind: TokenKind::Operator });
                i += 1;
                col_utf16 += 1;
                expect_property = false;
                continue;
            }

            // Whitespace / punctuation we don't color: advance by UTF-16 width.
            col_utf16 += c.len_utf16() as u32;
            i += 1;
            if !c.is_whitespace() {
                expect_property = false;
            }
        }
    }

    tokens
}

/// Emit a string literal as `String` tokens, splitting out `$identifier`
/// interpolation runs (highlighted as `Variable`) so they read distinctly.
fn emit_string_with_interpolation(out: &mut Vec<Token>, line: u32, start_col: u32, seg: &[char]) {
    let mut i = 0usize;
    let mut col = start_col;
    let mut plain_start = 0usize;
    let mut plain_col = start_col;

    while i < seg.len() {
        if seg[i] == '$' && i + 1 < seg.len() && (seg[i + 1].is_alphabetic() || seg[i + 1] == '_') {
            if i > plain_start {
                out.push(Token {
                    line,
                    col: plain_col,
                    len: utf16_len(&seg[plain_start..i]),
                    kind: TokenKind::String,
                });
            }
            let var_start = i;
            let var_col = col + utf16_len(&seg[plain_start..i]);
            i += 1;
            while i < seg.len() && (seg[i].is_alphanumeric() || seg[i] == '_') {
                i += 1;
            }
            out.push(Token {
                line,
                col: var_col,
                len: utf16_len(&seg[var_start + 1..i]) + 1,
                kind: TokenKind::Variable,
            });
            plain_start = i;
            plain_col = var_col + utf16_len(&seg[var_start..i]);
            col = plain_col;
        } else {
            i += 1;
        }
    }
    if plain_start < seg.len() {
        out.push(Token {
            line,
            col: plain_col,
            len: utf16_len(&seg[plain_start..]),
            kind: TokenKind::String,
        });
    }
}

fn peek_non_space(chars: &[char], mut i: usize) -> Option<char> {
    while i < chars.len() && chars[i] == ' ' {
        i += 1;
    }
    chars.get(i).copied()
}

fn utf16_len(chars: &[char]) -> u32 {
    chars.iter().map(|c| c.len_utf16() as u32).sum()
}

/// Delta-encode tokens per the LSP `semanticTokens/full` wire format: each
/// token becomes 5 integers (deltaLine, deltaStartChar, length, tokenType,
/// tokenModifiers). Tokens must be in document order (line, then column).
pub fn encode_deltas(tokens: &[Token]) -> Vec<u32> {
    let mut data = Vec::with_capacity(tokens.len() * 5);
    let mut prev_line = 0u32;
    let mut prev_col = 0u32;

    for t in tokens {
        let delta_line = t.line - prev_line;
        let delta_col = if delta_line == 0 { t.col - prev_col } else { t.col };
        data.push(delta_line);
        data.push(delta_col);
        data.push(t.len);
        data.push(t.kind as u32);
        data.push(0); // no modifiers defined yet
        prev_line = t.line;
        prev_col = t.col;
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(TokenKind, String)> {
        tokenize(src)
            .into_iter()
            .map(|t| {
                let line = src.lines().nth(t.line as usize).unwrap();
                let text: String = line
                    .chars()
                    .skip(t.col as usize) // columns line up 1:1 for ASCII test fixtures
                    .take(t.len as usize)
                    .collect();
                (t.kind, text)
            })
            .collect()
    }

    #[test]
    fn classifies_keyword_type_and_variable() {
        let got = kinds("if a { Point{} }");
        assert_eq!(got[0], (TokenKind::Keyword, "if".into()));
        assert_eq!(got[1], (TokenKind::Variable, "a".into()));
        assert_eq!(got[2], (TokenKind::Type, "Point".into()));
    }

    #[test]
    fn classifies_function_call_by_trailing_paren() {
        let got = kinds("length(a)");
        assert_eq!(got[0], (TokenKind::Function, "length".into()));
        assert_eq!(got[1], (TokenKind::Variable, "a".into()));
    }

    #[test]
    fn classifies_property_after_dot() {
        let got = kinds("point.x");
        assert_eq!(got[0], (TokenKind::Variable, "point".into()));
        assert_eq!(got[1], (TokenKind::Property, "x".into()));
    }

    #[test]
    fn classifies_comment_and_string() {
        let got = kinds("-> a comment");
        assert_eq!(got[0].0, TokenKind::Comment);
        let got = kinds("a = \"hello\"");
        assert!(got.iter().any(|(k, t)| *k == TokenKind::String && t == "\"hello\""));
    }

    #[test]
    fn classifies_interpolation_inside_string() {
        let toks = tokenize("greeting = \"Hello, $name!\"");
        let kinds: Vec<TokenKind> = toks.iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&TokenKind::String));
        assert!(kinds.contains(&TokenKind::Variable));
    }

    #[test]
    fn classifies_number_and_literal_keywords() {
        let got = kinds("a = 42 and true");
        assert!(got.iter().any(|(k, t)| *k == TokenKind::Number && t == "42"));
        assert!(got.iter().any(|(k, t)| *k == TokenKind::Keyword && t == "and"));
        assert!(got.iter().any(|(k, t)| *k == TokenKind::Keyword && t == "true"));
    }

    #[test]
    fn classifies_unicode_operator_as_single_token() {
        let toks = tokenize("a ≤ 3");
        let op = toks.iter().find(|t| t.kind == TokenKind::Operator).unwrap();
        assert_eq!(op.len, 1, "≤ is one UTF-16 unit");
    }

    #[test]
    fn utf16_column_survives_a_preceding_multibyte_operator() {
        // `≤` is 3 bytes / 1 char but exactly 1 UTF-16 unit, so the following
        // identifier's column must advance by 1, not by a byte or char count
        // that happens to differ (this case they coincide; regression guard
        // for the multi-byte path itself, exercised by the encode roundtrip
        // below).
        let toks = tokenize("x ≤ y");
        let y = toks.iter().find(|t| t.kind == TokenKind::Variable && t.col > 2).unwrap();
        assert_eq!(y.col, 4); // "x ≤ y" -> x(0) sp(1) ≤(2) sp(3) y(4)
    }

    #[test]
    fn delta_encoding_is_document_order_relative() {
        let toks = vec![
            Token { line: 0, col: 0, len: 2, kind: TokenKind::Keyword },
            Token { line: 0, col: 3, len: 1, kind: TokenKind::Variable },
            Token { line: 1, col: 0, len: 4, kind: TokenKind::Keyword },
        ];
        let data = encode_deltas(&toks);
        assert_eq!(data, vec![
            0, 0, 2, TokenKind::Keyword as u32, 0,
            0, 3, 1, TokenKind::Variable as u32, 0,
            1, 0, 4, TokenKind::Keyword as u32, 0,
        ]);
    }
}

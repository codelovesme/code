use crate::span::Located;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    Number(f64),
    Str(String),
    True,
    False,
    Null,
    /// `=` — both the assignment separator in a `let`/reassignment statement
    /// *and* the equality operator inside an expression. Not ambiguous: a
    /// statement's `[let] IDENT =` prefix is consumed before expression
    /// parsing ever starts, and no statement begins with a bare expression,
    /// so every `=` the expression grammar sees is an equality.
    Equals,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    LParen,
    RParen,
    Colon,
    Comma,
    Dot,
    Plus,
    /// `+=` — the language's only multi-character operator, and only ever a
    /// statement form (`name += expr`), never part of an expression. The
    /// parser rewrites it into `name = name + expr`, so it never reaches
    /// either backend.
    PlusEq,
    Minus,
    Star,
    Slash,
    /// `≠`
    NotEq,
    Lt,
    Gt,
    /// `≤`
    Le,
    /// `≥`
    Ge,
    And,
    Or,
    Not,
    Assert,
    If,
    Let,
    Loop,
    Over,
    Break,
    Continue,
    Link,
    As,
    Export,
    Emit,
    To,
    /// The only valid `emit` target today. A reserved word (not just a
    /// special-cased identifier) specifically so `let core = ...` is
    /// rejected at parse time rather than silently shadowing it.
    Core,
    /// `get` — binds an `emit`'s result to a name. Always declares a fresh
    /// binding, shadowing like `let` — never a reassignment.
    Get,
    /// `is` — the type-test operator: `expr is ClassName` asks whether
    /// `expr` is a particle of that class (see `Expr::Is`).
    Is,
    /// Statement separator — a newline or `;`. Blank lines never produce one
    /// (see `tokenize`: consecutive separators are collapsed).
    Newline,
    Eof,
}

/// Tokens plus where each one started — the two are always produced
/// together, so they travel together rather than as two arguments every
/// caller has to keep in step.
pub struct Lexed {
    pub tokens: Vec<Token>,
    /// Char offset into the source where `tokens[i]` starts, same length as
    /// `tokens`. Char, not byte, so a multi-byte operator earlier on the line
    /// doesn't skew the column — see `span::render`.
    pub starts: Vec<u32>,
    /// Char offset just past `tokens[i]` — `starts[i]..ends[i]` slices the
    /// token's exact source text. Same length as `tokens` and `starts`. The
    /// two synthetic end-of-input tokens (a trailing `Newline`, then `Eof`)
    /// are zero-width: their `ends` equals their `starts`, since neither
    /// covers real source text. Consumers that want a token's literal
    /// spelling (`crates/code-lsp`'s semantic tokens; the formatter
    /// described in `docs/todo/formatter.md`) slice `src` with this instead
    /// of re-deriving it from the `Token` payload, which is lossy (a
    /// `Number` is an `f64`, not the digits as written; `+=` is rewritten
    /// away by the parser, never even reaching this struct's consumers).
    pub ends: Vec<u32>,
}

pub fn tokenize(src: &str) -> Result<Lexed, Located> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut tokens = Vec::new();
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    let mut last_was_separator = true; // suppress a leading Newline

    while i < chars.len() {
        let c = chars[i];
        // Whitespace and comments `continue` before reaching any push below,
        // so this is always the first char of whatever token is pushed next.
        let start = i;

        if c == '\n' || c == ';' {
            if !last_was_separator {
                tokens.push(Token::Newline);
                starts.push(start as u32);
                ends.push(start as u32 + 1);
                last_was_separator = true;
            }
            i += 1;
            continue;
        }

        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // `--` line comment. A lone `-` is the subtraction/negation operator
        // (below), so this has to be checked first.
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // `!` used to begin `!=`. Now that inequality is `≠` it has no
        // meaning at all, and a bare `!` is overwhelmingly likely to be
        // someone reaching for the operator that moved.
        if c == '!' {
            return Err(Located::at(
                i,
                "unexpected character '!' (inequality is '≠')",
            ));
        }

        // `+=`, checked before the single-character table below so the `+`
        // isn't consumed on its own. The only two-character operator there
        // is: the comparison operators are each exactly one character, which
        // is what `==`/`<=`/`>=` are rejected for.
        if c == '+' && chars.get(i + 1) == Some(&'=') {
            tokens.push(Token::PlusEq);
            starts.push(start as u32);
            ends.push(start as u32 + 2);
            last_was_separator = false;
            i += 2;
            continue;
        }

        if let Some(tok) = match c {
            '[' => Some(Token::LBracket),
            ']' => Some(Token::RBracket),
            '{' => Some(Token::LBrace),
            '}' => Some(Token::RBrace),
            '(' => Some(Token::LParen),
            ')' => Some(Token::RParen),
            ':' => Some(Token::Colon),
            ',' => Some(Token::Comma),
            '.' => Some(Token::Dot),
            '+' => Some(Token::Plus),
            '-' => Some(Token::Minus),
            '*' => Some(Token::Star),
            '/' => Some(Token::Slash),
            // Every comparison operator is exactly one character — there
            // are no multi-character operators in the language at all.
            '=' => Some(Token::Equals),
            '<' => Some(Token::Lt),
            '>' => Some(Token::Gt),
            '≠' => Some(Token::NotEq),
            '≤' => Some(Token::Le),
            '≥' => Some(Token::Ge),
            _ => None,
        } {
            tokens.push(tok);
            starts.push(start as u32);
            ends.push(start as u32 + 1);
            last_was_separator = false;
            i += 1;
            continue;
        }

        if c == '"' {
            i += 1;
            let mut s = String::new();
            loop {
                match chars.get(i) {
                    // Points at the opening quote, not the end of the file:
                    // the quote is what the reader has to go find.
                    None => return Err(Located::at(start, "unterminated string literal")),
                    Some('"') => {
                        i += 1;
                        break;
                    }
                    Some('\\') => {
                        i += 1;
                        match chars.get(i) {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some('"') => s.push('"'),
                            Some('\\') => s.push('\\'),
                            Some(other) => {
                                return Err(Located::at(
                                    i - 1,
                                    format!("unknown escape '\\{other}'"),
                                ))
                            }
                            None => return Err(Located::at(start, "unterminated string literal")),
                        }
                        i += 1;
                    }
                    Some(ch) => {
                        s.push(*ch);
                        i += 1;
                    }
                }
            }
            tokens.push(Token::Str(s));
            starts.push(start as u32);
            ends.push(i as u32);
            last_was_separator = false;
            continue;
        }

        if c.is_ascii_digit() {
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let n: f64 = text
                .parse()
                .map_err(|_| Located::at(start, format!("invalid number literal '{text}'")))?;
            tokens.push(Token::Number(n));
            starts.push(start as u32);
            ends.push(i as u32);
            last_was_separator = false;
            continue;
        }

        if c.is_alphabetic() || c == '_' {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let tok = match text.as_str() {
                "true" => Token::True,
                "false" => Token::False,
                "null" => Token::Null,
                "and" => Token::And,
                "or" => Token::Or,
                "not" => Token::Not,
                "assert" => Token::Assert,
                "if" => Token::If,
                "let" => Token::Let,
                "loop" => Token::Loop,
                "over" => Token::Over,
                "break" => Token::Break,
                "continue" => Token::Continue,
                "link" => Token::Link,
                "as" => Token::As,
                "export" => Token::Export,
                "emit" => Token::Emit,
                "to" => Token::To,
                "core" => Token::Core,
                "get" => Token::Get,
                "is" => Token::Is,
                _ => Token::Ident(text),
            };
            tokens.push(tok);
            starts.push(start as u32);
            ends.push(i as u32);
            last_was_separator = false;
            continue;
        }

        return Err(Located::at(i, format!("unexpected character '{c}'")));
    }

    // Both synthetic: they stand at the end of the source, which is exactly
    // where an "unexpected end of input" error wants to point. Zero-width —
    // see `Lexed::ends`.
    if !last_was_separator {
        tokens.push(Token::Newline);
        starts.push(chars.len() as u32);
        ends.push(chars.len() as u32);
    }
    tokens.push(Token::Eof);
    starts.push(chars.len() as u32);
    ends.push(chars.len() as u32);
    Ok(Lexed {
        tokens,
        starts,
        ends,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `starts[i]..ends[i]` must slice back exactly the substring that
    /// produced `tokens[i]` — the property every `Lexed::ends` consumer
    /// relies on (see its doc comment).
    #[test]
    fn ends_slice_back_the_source_text() {
        let src = "let n += 1.50 -- a comment\nemit Foo {} to core get t\n\"a\\nb\"";
        let lexed = tokenize(src).unwrap();
        let chars: Vec<char> = src.chars().collect();
        let mut texts = Vec::new();
        for (start, end) in lexed.starts.iter().zip(&lexed.ends) {
            let text: String = chars[*start as usize..*end as usize].iter().collect();
            texts.push(text);
        }
        assert_eq!(
            texts,
            vec![
                "let",
                "n",
                "+=",
                "1.50",
                "\n",
                "emit",
                "Foo",
                "{",
                "}",
                "to",
                "core",
                "get",
                "t",
                "\n",
                "\"a\\nb\"",
                "",
                "",
            ],
        );
    }
}

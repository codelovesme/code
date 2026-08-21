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
    Link,
    As,
    Export,
    /// Statement separator — a newline or `;`. Blank lines never produce one
    /// (see `tokenize`: consecutive separators are collapsed).
    Newline,
    Eof,
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut tokens = Vec::new();
    let mut last_was_separator = true; // suppress a leading Newline

    while i < chars.len() {
        let c = chars[i];

        if c == '\n' || c == ';' {
            if !last_was_separator {
                tokens.push(Token::Newline);
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
            return Err("unexpected character '!' (inequality is '≠')".to_string());
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
            last_was_separator = false;
            i += 1;
            continue;
        }

        if c == '"' {
            i += 1;
            let mut s = String::new();
            loop {
                match chars.get(i) {
                    None => return Err("unterminated string literal".to_string()),
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
                            Some(other) => return Err(format!("unknown escape '\\{other}'")),
                            None => return Err("unterminated string literal".to_string()),
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
            last_was_separator = false;
            continue;
        }

        if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let n: f64 = text
                .parse()
                .map_err(|_| format!("invalid number literal '{text}'"))?;
            tokens.push(Token::Number(n));
            last_was_separator = false;
            continue;
        }

        if c.is_alphabetic() || c == '_' {
            let start = i;
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
                "link" => Token::Link,
                "as" => Token::As,
                "export" => Token::Export,
                _ => Token::Ident(text),
            };
            tokens.push(tok);
            last_was_separator = false;
            continue;
        }

        return Err(format!("unexpected character '{c}'"));
    }

    if !last_was_separator {
        tokens.push(Token::Newline);
    }
    tokens.push(Token::Eof);
    Ok(tokens)
}

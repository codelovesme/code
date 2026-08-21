#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    Number(f64),
    Str(String),
    True,
    False,
    Null,
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
    EqEq,
    NotEq,
    Lt,
    Gt,
    Le,
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

        // Two-character operators — each falls back to a one-character
        // token (or an error, for a bare `!`) when the second `=` isn't
        // there.
        let two_char = match (c, chars.get(i + 1)) {
            ('=', Some('=')) => Some((Token::EqEq, 2)),
            ('!', Some('=')) => Some((Token::NotEq, 2)),
            ('<', Some('=')) => Some((Token::Le, 2)),
            ('>', Some('=')) => Some((Token::Ge, 2)),
            ('=', _) => Some((Token::Equals, 1)),
            ('<', _) => Some((Token::Lt, 1)),
            ('>', _) => Some((Token::Gt, 1)),
            _ => None,
        };
        if let Some((tok, len)) = two_char {
            tokens.push(tok);
            last_was_separator = false;
            i += len;
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

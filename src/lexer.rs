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
    Colon,
    Comma,
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

        // `--` line comment, chosen to avoid colliding with a future `-`
        // (negation/subtraction) operator on a single dash.
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        if let Some(tok) = match c {
            '=' => Some(Token::Equals),
            '[' => Some(Token::LBracket),
            ']' => Some(Token::RBracket),
            '{' => Some(Token::LBrace),
            '}' => Some(Token::RBrace),
            ':' => Some(Token::Colon),
            ',' => Some(Token::Comma),
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

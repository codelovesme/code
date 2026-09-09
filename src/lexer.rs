use crate::span::Located;

/// One piece of an interpolated string, in source order: either literal text
/// (already unescaped) or a variable to splice in.
///
/// `Var` carries a name and the fields read from it, so `"$user.name"` is the
/// name and not the object followed by the text ".name". It used to be the
/// second thing, silently: `$many.value` rendered the whole of `many` and
/// then four literal characters, with nothing said. A reader writing
/// `$user.name` means the field, every time.
#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    Lit(String),
    Var { name: String, fields: Vec<String> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    Number(f64),
    Str(String),
    /// A double-quoted string that contained at least one `$name`. Splitting
    /// happens here rather than in the parser because the quote-scanning loop
    /// already owns string internals — escapes included. A string with no `$`
    /// stays a plain `Str`, so every existing use site (a `link` path, an
    /// object key) keeps rejecting interpolation for free.
    InterpStr(Vec<StringPart>),
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
    /// `+=` — only ever a statement form (`name += expr`), never part of an
    /// expression. The parser rewrites it into `name = name + expr`, so it
    /// never reaches either backend.
    PlusEq,
    /// `=>` — separates a handler's class name from its body
    /// (`Greet => { ... }`). One of the two multi-character operators, the
    /// other being `+=`.
    Arrow,
    Minus,
    Star,
    Slash,
    /// `∈`
    In,
    /// `∉` — `x ∉ Name` is `not (x ∈ Name)`, and nothing more: the parser
    /// builds exactly that tree, so both backends inherit the answer from
    /// `∈` rather than having a second rule to keep in step.
    ///
    /// One character, like `≠` is one character to `=`'s one. Spelling it
    /// `not x ∈ Name` worked but read badly — `not` binds looser than `∈`,
    /// so the eye has to work out that the whole membership is what is being
    /// negated rather than `x`.
    NotIn,
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
    Loop,
    Over,
    Break,
    Continue,
    Link,
    /// `unlink <expr>` — closes a module opened by a `link` that ran
    /// inside a handler. A reserved word rather than a core particle so it
    /// reads as the exact mirror of `link`, and so `let unlink = ...` is a
    /// parse error instead of quietly shadowing it.
    Unlink,
    As,
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
    /// `this` — the `emit ... to this` target: a handler the program
    /// defines itself. A keyword rather than an ordinary name, so it can
    /// never also be a variable.
    This,
    /// `base` — the `emit ... to base` target: the handlers defined by
    /// whoever `link`ed the module this statement sits in. Reserved for the
    /// same reason `this` is — it must never be a variable name.
    Base,
    /// `return` — a handler body's early exit with a result.
    Return,
    /// A block's body begins: the line after a block header is indented
    /// further than the header was. Zero-width and synthetic, like `Newline`
    /// — it stands at the first real character of the indented line.
    ///
    /// **Only outside brackets.** Inside `{`, `[` or `(` the lexer is
    /// counting a bracket, not a block, so indentation there is the author's
    /// own layout and produces nothing. That is what keeps a multi-line
    /// object literal free to indent however it reads best.
    Indent,
    /// A block's body ends: a line indented less than the one before it. One
    /// per level closed, and one per open level at end of input, so every
    /// `Indent` is matched.
    Dedent,
    /// Statement separator — a newline. Blank lines never produce one (see
    /// `tokenize`: consecutive separators are collapsed).
    ///
    /// A newline is the only spelling. `;` was a second one until 1.4.0,
    /// carried over from languages that need it and never used once in this
    /// repository's own corpus; once a `}` could end a statement there was
    /// nothing left that required it, and a redundant spelling is the thing
    /// this language keeps removing.
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

/// Whether everything before `at` on its line is whitespace — i.e. `at` is
/// the first thing written on that line.
fn line_so_far_is_blank(chars: &[char], at: usize) -> bool {
    chars[..at]
        .iter()
        .rev()
        .take_while(|&&c| c != '\n')
        .all(|c| c.is_whitespace())
}

pub fn tokenize(src: &str) -> Result<Lexed, Located> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut tokens = Vec::new();
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    let mut last_was_separator = true; // suppress a leading Newline
                                       // How deep inside `{`/`[`/`(` we are. Indentation is only structure at
                                       // depth 0; inside a bracket the closer is what ends the construct, so a
                                       // multi-line literal lays itself out however its author likes.
    let mut bracket_depth: usize = 0;
    // Columns of the block levels currently open, outermost first. The 0 is
    // the file's own top level and is never popped.
    let mut indents: Vec<usize> = vec![0];

    while i < chars.len() {
        let c = chars[i];
        // Whitespace and comments `continue` before reaching any push below,
        // so this is always the first char of whatever token is pushed next.
        let start = i;

        if c == '\n' {
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

        // A line that *opens* with `--` is a comment from before 1.4.0.
        //
        // Caught here rather than in the parser because the parser never sees
        // it: a comment's prose is not a token stream, and this repository's
        // own comments are full of backticks, so the lexer would fail on the
        // text long before anything could recognise the `--`. Answering at
        // the marker means the message survives whatever follows it.
        //
        // Only at the start of a line, and that is the whole safety argument.
        // `--` is no longer special anywhere else: `5--1` is `5 - -1`, which
        // is exactly the ambiguity removing the old marker resolved, so
        // refusing `--` outright the way `!` and `;` are refused would take
        // real arithmetic with it. What this gives up is a `--2` written
        // alone on a continuation line inside a multi-line literal, where
        // double negation and an old comment look identical; `- -2` says it
        // without the guess.
        if c == '-' && chars.get(i + 1) == Some(&'-') && line_so_far_is_blank(&chars, i) {
            return Err(Located::at(
                i,
                "`--` is not a comment — the marker is `|`. (It changed in 1.4.0; \
                 `--` now reads as two minus signs.)",
            ));
        }

        // `|` line comment. One character, because unlike the `--` it
        // replaced there is nothing to tell it apart from: `|` is not an
        // operator here and never was, so the second character `--` needed
        // to escape `-`'s own meaning would buy nothing.
        if c == '|' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // The first real character of a line, outside any bracket: this is
        // where a block opens or closes. Comment-only and blank lines never
        // reach here — both `continue` above — so neither disturbs the
        // structure, which is what lets a comment sit at any indentation.
        if bracket_depth == 0 && line_so_far_is_blank(&chars, i) {
            let line_start = chars[..i]
                .iter()
                .rposition(|&c| c == '\n')
                .map(|n| n + 1)
                .unwrap_or(0);
            // A tab is not a width, it is a request that every reader's
            // editor agree about one — and they do not. Refused at the
            // marker rather than silently counted as one column.
            if let Some(offset) = chars[line_start..i].iter().position(|&c| c == '\t') {
                return Err(Located::at(
                    line_start + offset,
                    "a tab in a line's indentation — indent with spaces, since \
                     a block's depth is now what a line means",
                ));
            }
            let indent = i - line_start;
            let current = *indents.last().expect("the top level is never popped");
            if indent > current {
                indents.push(indent);
                tokens.push(Token::Indent);
                starts.push(i as u32);
                ends.push(i as u32);
            } else {
                while indent < *indents.last().expect("the top level is never popped") {
                    indents.pop();
                    tokens.push(Token::Dedent);
                    starts.push(i as u32);
                    ends.push(i as u32);
                }
                // Closing levels landed somewhere between two of them: the
                // line belongs to no block that is open, which is a layout
                // the reader cannot resolve either.
                if indent != *indents.last().expect("the top level is never popped") {
                    return Err(Located::at(
                        i,
                        "this line's indentation matches no block that is open",
                    ));
                }
            }
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

        // `;` was a second spelling of the statement separator until 1.4.0.
        // Anyone typing one is reaching for a separator, so say which one
        // there is rather than reporting an unknown character.
        if c == ';' {
            return Err(Located::at(
                i,
                "unexpected character ';' — a newline separates statements, \
                 and a '}' ends the last one in a block",
            ));
        }

        // The two-character operators, checked before the single-character
        // table below so their first character isn't consumed on its own.
        // There are exactly two — the comparison operators are each one
        // character, which is what `==`/`<=`/`>=` are rejected for.
        if c == '+' && chars.get(i + 1) == Some(&'=') {
            tokens.push(Token::PlusEq);
            starts.push(start as u32);
            ends.push(start as u32 + 2);
            last_was_separator = false;
            i += 2;
            continue;
        }

        // `=>` introduces a handler body. Checked before `=` so the arrow
        // never decays into an equality followed by a stray `>`.
        if c == '=' && chars.get(i + 1) == Some(&'>') {
            tokens.push(Token::Arrow);
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
            // The membership operator: `x ∈ String` asks, and
            // `let a ∈ String = …` says. One character, like every other
            // operator in the language.
            '∈' => Some(Token::In),
            '∉' => Some(Token::NotIn),
            '≤' => Some(Token::Le),
            '≥' => Some(Token::Ge),
            _ => None,
        } {
            match tok {
                Token::LBrace | Token::LBracket | Token::LParen => bracket_depth += 1,
                // Saturating because an unmatched closer is the parser's
                // error to report, with the context to say what was expected
                // — the lexer only has to not go negative on the way there.
                Token::RBrace | Token::RBracket | Token::RParen => {
                    bracket_depth = bracket_depth.saturating_sub(1)
                }
                _ => {}
            }
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
            let mut parts: Vec<StringPart> = Vec::new();
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
                            // The one escape that exists only because `$` is
                            // otherwise mandatory-interpolation: without it a
                            // literal dollar would be unwritable, not merely
                            // awkward (`"costs $5"` would be a lex error).
                            Some('$') => s.push('$'),
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
                    Some('$') => {
                        let dollar = i;
                        i += 1;
                        let name_start = i;
                        if !matches!(chars.get(i), Some(c) if c.is_alphabetic() || *c == '_') {
                            return Err(Located::at(
                                dollar,
                                "'$' in a string must start an interpolation ($name); \
                                 write '\\$' for a literal dollar sign",
                            ));
                        }
                        while matches!(chars.get(i), Some(c) if c.is_alphanumeric() || *c == '_') {
                            i += 1;
                        }
                        let name: String = chars[name_start..i].iter().collect();

                        // `.field`, as many as are written. A dot followed by
                        // anything else is literal text — `"$total."` ends a
                        // sentence, and a reader who meant a field would have
                        // written one.
                        let mut fields = Vec::new();
                        while matches!(chars.get(i), Some('.'))
                            && matches!(chars.get(i + 1), Some(c) if c.is_alphabetic() || *c == '_')
                        {
                            i += 1;
                            let field_start = i;
                            while matches!(chars.get(i), Some(c) if c.is_alphanumeric() || *c == '_')
                            {
                                i += 1;
                            }
                            fields.push(chars[field_start..i].iter().collect());
                        }

                        if !s.is_empty() {
                            parts.push(StringPart::Lit(std::mem::take(&mut s)));
                        }
                        parts.push(StringPart::Var { name, fields });
                    }
                    Some(ch) => {
                        s.push(*ch);
                        i += 1;
                    }
                }
            }
            let tok = if parts.is_empty() {
                Token::Str(s)
            } else {
                if !s.is_empty() {
                    parts.push(StringPart::Lit(s));
                }
                Token::InterpStr(parts)
            };
            tokens.push(tok);
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
                "loop" => Token::Loop,
                "over" => Token::Over,
                "break" => Token::Break,
                "continue" => Token::Continue,
                "link" => Token::Link,
                "unlink" => Token::Unlink,
                "as" => Token::As,
                "emit" => Token::Emit,
                "to" => Token::To,
                "core" => Token::Core,
                "get" => Token::Get,
                "is" => Token::Is,
                "this" => Token::This,
                "base" => Token::Base,
                "return" => Token::Return,
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
    // End of input closes every block still open, so the parser never has to
    // treat the last one differently from the rest.
    while indents.len() > 1 {
        indents.pop();
        tokens.push(Token::Dedent);
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

    /// Indentation is structure outside brackets and layout inside them —
    /// the one rule the whole design rests on.
    #[test]
    fn indentation_is_structure_outside_brackets_and_layout_inside() {
        let kinds = |src: &str| {
            tokenize(src)
                .unwrap()
                .tokens
                .into_iter()
                .filter(|t| matches!(t, Token::Indent | Token::Dedent))
                .collect::<Vec<_>>()
        };

        // One block, opened by the indent and closed at end of input.
        assert_eq!(
            kinds("if a\n    assert b\n"),
            vec![Token::Indent, Token::Dedent]
        );

        // Two levels, both closed by the single outdent back to column 0.
        assert_eq!(
            kinds("if a\n    if b\n        assert c\nassert d\n"),
            vec![Token::Indent, Token::Indent, Token::Dedent, Token::Dedent]
        );

        // Inside a literal the same shape produces nothing at all.
        assert_eq!(kinds("o = {\n    a = 1,\n    b = 2\n}\n"), vec![]);

        // Blank and comment-only lines do not disturb the structure, which
        // is what lets a comment sit wherever it reads best.
        assert_eq!(
            kinds("if a\n\n  | note\n    assert b\n"),
            vec![Token::Indent, Token::Dedent]
        );
    }

    /// A line that closes past one open level but lands on none of them is
    /// refused rather than guessed at.
    #[test]
    fn an_indentation_matching_no_open_block_is_refused() {
        let Err(err) = tokenize("if a\n        assert b\n    assert c\n") else {
            panic!("that layout matches no open block, so it must not lex");
        };
        assert!(
            err.msg.contains("matches no block that is open"),
            "{}",
            err.msg
        );
    }

    /// `starts[i]..ends[i]` must slice back exactly the substring that
    /// produced `tokens[i]` — the property every `Lexed::ends` consumer
    /// relies on (see its doc comment).
    #[test]
    fn ends_slice_back_the_source_text() {
        let src = "n += 1.50 | a comment\nemit Foo {} to core get t\n\"a\\nb\"";
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

//! One canonical layout for `.code` source.
//!
//! Formats the **token stream**, not the AST, and that is the decision the
//! rest of this file follows from. The AST is desugared by design: `lexer.rs`
//! drops comments entirely, the parser rewrites `n += 1` into `n = n + 1` and
//! `Timestamp {}` into an object with a `_class` field, and a `Number` is an
//! `f64` rather than the digits someone typed. Printing that back out would
//! delete every `--` comment in `tests/`, which is most of the language's
//! documentation. Teaching the AST to carry comments is the "heavy and
//! dependent everywhere" change ruled out for spans.
//!
//! So this reads `Lexed` plus the original text, and every token it emits is
//! a slice of that text (`starts[i]..ends[i]`). Literals keep the author's
//! spelling, escapes are never re-encoded, particle sugar survives because it
//! was never desugared — and the formatter has no opinion about semantics at
//! all, which is what makes the safety properties in
//! `tests/format_fixtures.rs` provable rather than hoped for.
//!
//! **Hard line breaks are the author's.** There is no max width and no
//! re-flow: a `{ x = 1 }` written inline stays inline, a multi-line array
//! stays multi-line. That is the rule keeping this small, and it also
//! dissolves the one genuinely awkward question in the grammar — `{` opens
//! both a block and an object literal, and the token stream cannot always
//! tell which. Depth counting does not care.
//!
//! See `docs/todo/formatter.md` for the rules and what was deliberately left
//! out of v1.

use crate::lexer::{self, Token};
use crate::span::Located;

/// Lays `src` out canonically. Fails only where the file could not be read
/// completely: a lex error, or a parse error.
///
/// The parse is a guard, not a source of information — nothing below looks at
/// the `Program`. Lexing alone is enough to lay a file out, but an unbalanced
/// brace would silently reindent everything after it into nonsense, so a file
/// that does not parse is one this must refuse to touch.
pub fn format(src: &str) -> Result<String, Located> {
    let lexed = lexer::tokenize(src)?;
    crate::parser::parse(&lexed)?;

    let chars: Vec<char> = src.chars().collect();
    let mut f = Formatter {
        chars: &chars,
        lines: Vec::new(),
        line: String::new(),
        line_depth: 0,
        depth: 0,
        prev: None,
    };

    // The region before the first token, which is where a file's opening
    // comment block lives — every fixture in `tests/` starts with one.
    f.gap(0, lexed.starts.first().copied().unwrap_or(0) as usize);

    for (i, token) in lexed.tokens.iter().enumerate() {
        match token {
            // Both synthetic tokens stand at the end of input and cover no
            // text; the gap before `Eof` was already walked by the previous
            // iteration.
            Token::Eof => break,
            Token::Newline => f.finish_line(),
            _ => f.push_token(token, lexed.starts[i] as usize, lexed.ends[i] as usize),
        }
        let gap_start = lexed.ends[i] as usize;
        let gap_end = lexed.starts.get(i + 1).copied().unwrap_or(0) as usize;
        f.gap(gap_start, gap_end);
    }
    f.finish_line();

    Ok(f.render())
}

/// Whether a token can be the last thing in an operand — which is what makes
/// a following `[` a subscript rather than the start of an array literal.
fn ends_operand(token: &Token) -> bool {
    matches!(
        token,
        Token::Ident(_)
            | Token::Number(_)
            | Token::Str(_)
            | Token::InterpStr(_)
            | Token::True
            | Token::False
            | Token::Null
            | Token::RBracket
            | Token::RParen
            | Token::RBrace
    )
}

/// A line still being built, plus everything needed to indent it.
struct Formatter<'a> {
    chars: &'a [char],
    /// Finished lines. `None` is a blank line the author left, kept as a
    /// marker rather than a `String` so `render` can collapse runs of them
    /// without re-inspecting text.
    lines: Vec<Option<String>>,
    line: String,
    /// Depth when the current line started, which is what indents it — not
    /// `depth`, which may already have moved past an opener on this line.
    line_depth: usize,
    depth: usize,
    prev: Option<Token>,
}

impl Formatter<'_> {
    fn text(&self, start: usize, end: usize) -> String {
        self.chars[start..end].iter().collect()
    }

    fn push_token(&mut self, token: &Token, start: usize, end: usize) {
        let closer = matches!(token, Token::RBrace | Token::RBracket | Token::RParen);
        // Before choosing the indent: a line *beginning* with a closer sits at
        // the depth of whatever it closes, not one deeper.
        if closer {
            self.depth = self.depth.saturating_sub(1);
        }
        if self.line.is_empty() {
            self.line_depth = self.depth;
        } else if self.space_before(token) {
            self.line.push(' ');
        }
        self.line.push_str(&self.text(start, end));
        if matches!(token, Token::LBrace | Token::LBracket | Token::LParen) {
            self.depth += 1;
        }
        self.prev = Some(token.clone());
    }

    /// One space between tokens, with the exceptions the corpus already
    /// follows. Braces are the odd ones: `{ x = 1 }` and `Log { x = 1 }`
    /// keep an inner space, where `[1, 2]` and `f(x)` do not — which is the
    /// majority style in `tests/` by 229 to 51. A field's `=` takes a space
    /// on both sides, like every other `=`, which falls out of the default
    /// rather than needing a rule.
    fn space_before(&self, next: &Token) -> bool {
        let Some(prev) = &self.prev else {
            return false;
        };
        match (prev, next) {
            // `.` binds its two sides together: `point.x`, never `point . x`.
            (Token::Dot, _) | (_, Token::Dot) => false,
            (_, Token::Comma) => false,
            (Token::LBracket | Token::LParen, _) => false,
            (_, Token::RBracket | Token::RParen) => false,
            // `[` opens a literal (`let xs = [1, 2]`) or subscripts one
            // (`data.items[2]`), told apart by whether anything that could
            // end an operand precedes it. Only the literal takes a space.
            (_, Token::LBracket) if ends_operand(prev) => false,
            // Empty braces close up: `Ping {}`, not `Ping { }`. The inner
            // space exists to hold content off the braces, and there is none.
            (Token::LBrace, Token::RBrace) => false,
            // Unary minus binds to its operand (`-3.5`), binary does not
            // (`a - b`). Told apart by what precedes the minus, which is the
            // same test the parser makes.
            _ if self.prev_is_unary_minus() => false,
            _ => true,
        }
    }

    /// Whether the token just emitted was a *unary* `-`. Unary exactly when
    /// nothing could have ended an operand before it: an operator, an opener,
    /// a separator, a keyword, or the start of a line.
    fn prev_is_unary_minus(&self) -> bool {
        if !matches!(self.prev, Some(Token::Minus)) {
            return false;
        }
        // `self.line` ends with the `-` itself; what came before it decides.
        let before = self.line[..self.line.len() - 1].trim_end();
        let Some(last) = before.chars().last() else {
            return true; // line starts with it
        };
        if "([{,:=<>+-*/".contains(last) {
            return true;
        }
        // A word: unary after a keyword (`return -1`, `assert -1 = -1`),
        // binary after a value (`a - 1`).
        let word: String = before
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        matches!(
            word.as_str(),
            "let"
                | "assert"
                | "return"
                | "emit"
                | "if"
                | "loop"
                | "over"
                | "to"
                | "get"
                | "and"
                | "or"
                | "not"
                | "is"
                | "export"
                | "break"
                | "continue"
        )
    }

    fn finish_line(&mut self) {
        if self.line.is_empty() {
            self.lines.push(None);
        } else {
            let line = std::mem::take(&mut self.line);
            self.lines
                .push(Some(format!("{}{line}", "    ".repeat(self.line_depth))));
        }
        self.prev = None;
    }

    /// Walks the text between two tokens. By construction it holds nothing
    /// but whitespace, `;`, and `--` comments: anything else would have
    /// become a token, and a `--` inside a string literal is inside that
    /// token rather than out here.
    fn gap(&mut self, start: usize, end: usize) {
        let mut i = start;
        while i < end && i < self.chars.len() {
            let c = self.chars[i];
            if c == '\n' || c == ';' {
                // A separator the lexer had already counted (it emits one
                // `Newline` per run), so reaching one here means the author
                // left a blank line — or ended a comment line.
                self.finish_line();
                i += 1;
            } else if c == '-' && self.chars.get(i + 1) == Some(&'-') {
                let from = i;
                while i < end && i < self.chars.len() && self.chars[i] != '\n' {
                    i += 1;
                }
                let comment = self.text(from, i).trim_end().to_string();
                if self.line.is_empty() {
                    // Its own line: indented like code, flushed by the `\n`
                    // that follows it.
                    self.line_depth = self.depth;
                    self.line = comment;
                } else {
                    // Trailing a statement, two spaces off the code.
                    self.line.push_str("  ");
                    self.line.push_str(&comment);
                }
            } else {
                i += 1;
            }
        }
    }

    /// Blank-line policy, applied once over the finished lines: runs collapse
    /// to one, none at the start of the file, none just inside an opener or
    /// just before a closer, and exactly one trailing newline.
    fn render(&self) -> String {
        let mut out = String::new();
        let mut pending_blank = false;
        for line in &self.lines {
            let Some(text) = line else {
                pending_blank = true;
                continue;
            };
            let trimmed = text.trim_start();
            let closes =
                trimmed.starts_with('}') || trimmed.starts_with(']') || trimmed.starts_with(')');
            let opened = out
                .trim_end()
                .chars()
                .last()
                .is_some_and(|c| c == '{' || c == '[' || c == '(');
            if pending_blank && !out.is_empty() && !closes && !opened {
                out.push('\n');
            }
            pending_blank = false;
            out.push_str(text);
            out.push('\n');
        }
        out
    }
}

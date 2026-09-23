//! The `syntax` native module — `code` source as coloured spans.
//!
//! Handlers:
//!
//! - `Highlight { source, from?, until? }` — answers `Highlighted { lines }`:
//!   one entry per source line from `from` (default 0) up to, not
//!   including, `until` (default: the last line) — not `to`, which is a
//!   keyword. Each entry is a list of
//!   spans `{ col, len, kind }` in characters, left to right; `kind` is one
//!   of `comment string number keyword class property variable operator`.
//!   What no span covers is plain.
//!
//! An editor asks for the lines it shows, and gets back only those — the
//! whole source is lexed every time (the lexer is fast; a line's colour can
//! depend on lines far above it), but only the visible part crosses back.
//!
//! The classification is the language server's (`crates/code-lsp/src/
//! tokens.rs`), on the real lexer: one authority for what a keyword is.
//! Unlike the language server, source that does not lex still gets colour,
//! because a file being typed is mostly a file that does not lex yet:
//!
//! 1. the whole source, lexed — exact;
//! 2. failing that, each line lexed on its own, indentation stripped;
//! 3. failing that too, that line scanned by hand: `|` comments, strings,
//!    numbers, and words — a word is asked of the lexer whether it is a
//!    keyword, so even the fallback has no keyword list of its own.
//!
//! Colours degrade from exact to close; they never go out.

use code::lexer::{tokenize, Token};
use code_native::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Comment,
    String,
    Number,
    Keyword,
    Class,
    Property,
    Variable,
    Operator,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Comment => "comment",
            Kind::String => "string",
            Kind::Number => "number",
            Kind::Keyword => "keyword",
            Kind::Class => "class",
            Kind::Property => "property",
            Kind::Variable => "variable",
            Kind::Operator => "operator",
        }
    }
}

/// A span on one line: character column, character length, kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    col: usize,
    len: usize,
    kind: Kind,
}

/// Exhaustive on purpose, as in the language server: a token added to the
/// lexer fails this module's build instead of going uncoloured.
fn classify(tok: &Token, prev_dot: bool) -> Option<Kind> {
    use Token::*;
    Some(match tok {
        Str(_) | InterpStr(_) => Kind::String,
        Number(_) => Kind::Number,
        True | False | Null | And | Or | Not | Assert | If | Loop | Over | Break | Continue | Link | Unlink | As
        | Emit | To | Core | Get | Is | This | Base | Return => Kind::Keyword,
        Equals | Plus | PlusEq | Minus | Star | Slash | NotEq | Lt | Gt | Le | Ge | Arrow | In | NotIn => {
            Kind::Operator
        }
        Ident(_) if prev_dot => Kind::Property,
        Ident(name) if name.chars().next().is_some_and(char::is_uppercase) => Kind::Class,
        Ident(_) => Kind::Variable,
        LBracket | RBracket | LBrace | RBrace | LParen | RParen | Colon | Comma | Dot | Newline | Indent | Dedent
        | Eof => return None,
    })
}

/// Spans for every line of `src`, from the lexer, or `None` when it does
/// not lex. Comments are recovered from the gaps between tokens.
fn lexed_spans(src: &str, line_count: usize) -> Option<Vec<Vec<Span>>> {
    let lexed = tokenize(src).ok()?;
    let chars: Vec<char> = src.chars().collect();
    let mut lines = vec![Vec::new(); line_count.max(1)];
    let (mut line, mut col, mut idx, mut ti) = (0usize, 0usize, 0usize, 0usize);
    let mut prev_dot = false;
    let mut add = |line: usize, span: Span| {
        if let Some(l) = lines.get_mut(line) {
            l.push(span);
        }
    };
    while idx < chars.len() {
        if ti < lexed.tokens.len() && lexed.starts[ti] as usize == idx {
            let tok = &lexed.tokens[ti];
            let end = (lexed.ends[ti] as usize).min(chars.len());
            if let Some(kind) = classify(tok, prev_dot) {
                // A token is on one line; if one ever is not, it is cut there.
                let len = chars[idx..end].iter().take_while(|c| **c != '\n').count();
                if len > 0 {
                    add(line, Span { col, len, kind });
                }
            }
            prev_dot = matches!(tok, Token::Dot);
            while idx < end {
                if chars[idx] == '\n' {
                    line += 1;
                    col = 0;
                } else {
                    col += 1;
                }
                idx += 1;
            }
            ti += 1;
            continue;
        }
        // Zero-width tokens (Indent, Dedent, Eof) start where a real one may.
        if ti < lexed.tokens.len() && (lexed.starts[ti] as usize) < idx {
            ti += 1;
            continue;
        }
        match chars[idx] {
            '|' => {
                let start = col;
                while idx < chars.len() && chars[idx] != '\n' {
                    idx += 1;
                    col += 1;
                }
                add(line, Span { col: start, len: col - start, kind: Kind::Comment });
                prev_dot = false;
            }
            '\n' => {
                line += 1;
                col = 0;
                idx += 1;
            }
            _ => {
                col += 1;
                idx += 1;
            }
        }
    }
    Some(lines)
}

/// One line on its own: lexed with its indentation stripped, or scanned.
fn line_spans(line: &str) -> Vec<Span> {
    let indent = line.chars().take_while(|c| *c == ' ').count();
    let rest: String = line.chars().skip(indent).collect();
    if let Some(mut lines) = lexed_spans(&rest, 1) {
        let mut spans = lines.swap_remove(0);
        for s in &mut spans {
            s.col += indent;
        }
        return spans;
    }
    scanned_spans(line)
}

/// Is `word` a keyword? The lexer is asked, so there is no second list.
fn keyword(word: &str) -> bool {
    tokenize(word)
        .ok()
        .and_then(|l| l.tokens.into_iter().next())
        .is_some_and(|t| classify(&t, false) == Some(Kind::Keyword))
}

/// A line the lexer refuses (an unterminated string, a `!`), coloured by
/// hand — close to the lexer, never better than it.
fn scanned_spans(line: &str) -> Vec<Span> {
    let chars: Vec<char> = line.chars().collect();
    let mut spans = Vec::new();
    let mut i = 0;
    let mut prev_dot = false;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        if c == '|' {
            spans.push(Span { col: i, len: chars.len() - i, kind: Kind::Comment });
            break;
        } else if c == '"' {
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                i += if chars[i] == '\\' { 2 } else { 1 };
            }
            i = (i + 1).min(chars.len());
            spans.push(Span { col: start, len: i - start, kind: Kind::String });
        } else if c.is_ascii_digit() {
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            spans.push(Span { col: start, len: i - start, kind: Kind::Number });
        } else if c.is_alphabetic() || c == '_' {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let kind = if prev_dot {
                Kind::Property
            } else if keyword(&word) {
                Kind::Keyword
            } else if c.is_uppercase() {
                Kind::Class
            } else {
                Kind::Variable
            };
            spans.push(Span { col: start, len: i - start, kind });
        } else if "=+-*/<>≠≤≥∈∉".contains(c) {
            i += 1;
            if c == '=' && chars.get(i) == Some(&'>') || c == '+' && chars.get(i) == Some(&'=') {
                i += 1;
            }
            spans.push(Span { col: start, len: i - start, kind: Kind::Operator });
        } else {
            i += 1;
        }
        prev_dot = c == '.';
    }
    spans
}

/// Spans for lines `from..to` of `src`.
fn highlight(src: &str, from: usize, to: Option<usize>) -> Vec<Vec<Span>> {
    let lines: Vec<&str> = src.split('\n').collect();
    let to = to.unwrap_or(lines.len()).min(lines.len());
    if from >= to {
        return Vec::new();
    }
    match lexed_spans(src, lines.len()) {
        Some(all) => all.into_iter().skip(from).take(to - from).collect(),
        None => lines[from..to].iter().map(|l| line_spans(l)).collect(),
    }
}

fn answer(out: &mut CodeValue, lines: &[Vec<Span>]) {
    let mut rows = SlotBuffer::new(lines.len());
    for (i, spans) in lines.iter().enumerate() {
        let mut items = SlotBuffer::new(spans.len());
        for (j, s) in spans.iter().enumerate() {
            let mut fields = SlotBuffer::new(3);
            number(fields.slot_mut(0), s.col as f64);
            number(fields.slot_mut(1), s.len as f64);
            owned_str(fields.slot_mut(2), s.kind.name());
            object(items.slot_mut(j as i64), &[c"col", c"len", c"kind"], &mut fields);
            fields.release_all();
        }
        array(rows.slot_mut(i as i64), &mut items);
        items.release_all();
    }
    let mut list = CodeValue::zeroed();
    array(&mut list, &mut rows);
    rows.release_all();
    let mut buf = SlotBuffer::new(2);
    borrowed_str(buf.slot_mut(0), c"Highlighted");
    copy(buf.slot_mut(1), &list);
    release(&mut list);
    object(out, &[c"_class", c"lines"], &mut buf);
    buf.release_all();
}

fn handle_highlight(out: &mut CodeValue, particle: &CodeValue) {
    let Some(source) = read_field_str(particle, "source") else {
        return exception(out, "syntax", "Highlight needs `source`, a String");
    };
    let from = read_field_number(particle, "from").unwrap_or(0.0).max(0.0) as usize;
    let to = read_field_number(particle, "until").map(|t| t.max(0.0) as usize);
    answer(out, &highlight(source, from, to));
}

#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// # Safety
///
/// Both pointers must be valid for the duration of the call and laid out
/// per `code_abi.h` — the host guarantees this on every dispatch.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "syntax", |out| match read_field_str(particle, "_class") {
        Some("Highlight") => handle_highlight(out, particle),
        _ => null(out),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str, line: usize) -> Vec<(&'static str, String)> {
        let lines: Vec<&str> = src.split('\n').collect();
        let chars: Vec<char> = lines[line].chars().collect();
        highlight(src, line, Some(line + 1))[0]
            .iter()
            .map(|s| (s.kind.name(), chars[s.col..s.col + s.len].iter().collect()))
            .collect()
    }

    #[test]
    fn a_program_that_lexes() {
        let src = "| greet\nHello { who } =>\n  emit Print { value = \"hi $who\" } to con get _\n  n = p.size + 1\n";
        assert_eq!(kinds(src, 0), [("comment", "| greet".to_string())]);
        let l1 = kinds(src, 1);
        assert_eq!(l1[0], ("class", "Hello".into()));
        assert!(l1.contains(&("variable", "who".into())));
        assert!(l1.contains(&("operator", "=>".into())));
        let l2 = kinds(src, 2);
        assert_eq!(l2[0], ("keyword", "emit".into()));
        assert!(l2.contains(&("string", "\"hi $who\"".into())));
        assert!(l2.contains(&("keyword", "to".into())));
        let l3 = kinds(src, 3);
        assert!(l3.contains(&("property", "size".into())));
        assert!(l3.contains(&("number", "1".into())));
    }

    #[test]
    fn only_the_asked_lines_come_back() {
        let src = "a = 1\nb = 2\nc = 3";
        let got = highlight(src, 1, Some(2));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0][0], Span { col: 0, len: 1, kind: Kind::Variable });
        assert_eq!(highlight(src, 0, None).len(), 3);
        assert!(highlight(src, 5, None).is_empty());
    }

    #[test]
    fn source_that_does_not_lex_still_has_colour() {
        // An unterminated string on line 1 fails the whole file.
        let src = "  if x ≤ 3, return Y\nname = \"unterminated\n| note";
        assert!(tokenize(src).is_err());
        let l0 = kinds(src, 0);
        assert_eq!(l0[0], ("keyword", "if".into()));
        assert!(l0.contains(&("operator", "≤".into())));
        assert!(l0.contains(&("class", "Y".into())));
        let l1 = kinds(src, 1);
        assert_eq!(l1[0], ("variable", "name".into()));
        assert_eq!(l1[2], ("string", "\"unterminated".into()));
        assert_eq!(kinds(src, 2), [("comment", "| note".to_string())]);
    }

    #[test]
    fn the_scanner_asks_the_lexer_for_keywords() {
        let got: Vec<&str> = scanned_spans("loop x over xs ! emit").iter().map(|s| s.kind.name()).collect();
        assert_eq!(got, ["keyword", "variable", "keyword", "variable", "keyword"]);
    }
}

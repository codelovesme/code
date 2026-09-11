//! Execution tracing and replayable particle tests.
//!
//! The language has one reusable behavior abstraction — a handler answering a
//! particle — so there is exactly one boundary worth recording: an `emit`.
//! This module records those boundaries and nothing else. No clock, no
//! address, no pointer, no iteration counter: a trace of the same program over
//! the same inputs is byte-identical, which is what makes it diffable by an
//! agent and usable as a regression fixture.
//!
//! A recorded trace is also a test. `code replay` loads the program for real —
//! real handlers, real linked modules, no stand-ins — re-asks every root
//! `to this` boundary the trace recorded, and compares the answers by value.
//! A handler that starts answering something else is caught by the trace that
//! was taken when it answered correctly.
//!
//! ## Deliberate limits (deferred, not forgotten)
//!
//! - **Interpreted runs only.** `code run`/`code build` are untouched and
//!   tracing is opt-in through `code trace`, so no program changes meaning by
//!   existing. Compiled-path tracing needs a `runtime.c` half and its own
//!   parity fixtures; it is not part of this slice.
//! - **`emit` boundaries only.** A particle a module *pushes* (an inbound
//!   drain) and a question a host asks a guest (`ask_program`) do not pass
//!   through the `emit` statement and are not recorded yet.
//! - **A failed boundary records a null answer.** The error propagates as it
//!   always did; the trace just has nothing to show for that emit.
//! - **Replay re-asks root `to this` boundaries.** A nested boundary is
//!   reached by running its parent, and a `core`/module boundary is somebody
//!   else's contract, so both are reported as skipped rather than re-driven
//!   out of context.

use std::cell::RefCell;

use crate::value::Value;

/// The trace schema an agent reads. Additive fields only; a new field is a
/// new version.
pub const TRACE_SCHEMA_VERSION: u32 = 1;

/// One crossed particle boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceEvent {
    /// Position in call order, from zero. Pre-order: a boundary is numbered
    /// before the boundaries it causes.
    pub sequence: usize,
    /// How many handlers were already running when this emit ran. Zero is the
    /// program's own top level.
    pub depth: usize,
    /// Where the particle went: `this`, `core`, `base`, or `module:<alias>`.
    pub target: String,
    /// The particle's `_class`, repeated out of the particle so a reader can
    /// scan a trace without parsing every value.
    pub particle_class: String,
    pub particle: Value,
    /// What the boundary answered, or null when the emit failed.
    pub answer: Value,
}

/// Collects events while a program runs. Shared with the `Environment`, so it
/// keeps its own interior mutability rather than forcing a `&mut` through
/// every dispatch path.
#[derive(Debug, Default)]
pub struct Recorder {
    events: RefCell<Vec<TraceEvent>>,
}

impl Recorder {
    pub fn new() -> Self {
        Recorder::default()
    }

    /// Opens an event in call order and hands back its index, so the answer
    /// can be filled in once the boundary has actually answered.
    pub fn begin(&self, depth: usize, target: String, particle: &Value) -> usize {
        let mut events = self.events.borrow_mut();
        let sequence = events.len();
        events.push(TraceEvent {
            sequence,
            depth,
            target,
            particle_class: class_of(particle).to_string(),
            particle: particle.clone(),
            answer: Value::Null,
        });
        sequence
    }

    pub fn finish(&self, sequence: usize, answer: &Value) {
        if let Some(event) = self.events.borrow_mut().get_mut(sequence) {
            event.answer = answer.clone();
        }
    }

    pub fn events(&self) -> Vec<TraceEvent> {
        self.events.borrow().clone()
    }
}

/// A particle's `_class`, or `""` — the same reading `interpreter::class_of`
/// makes, kept here so the recorder does not need a private import.
fn class_of(particle: &Value) -> &str {
    match particle {
        Value::Object(fields) => match fields.iter().find(|(k, _)| k == "_class") {
            Some((_, Value::Str(class))) => class,
            _ => "",
        },
        _ => "",
    }
}

/// Renders a trace. Written by hand for the same reason the handler catalog
/// is: the interpreter-only wasm build must not grow a serde dependency, and
/// field order is part of the contract.
pub fn render_trace(entry: &str, events: &[TraceEvent]) -> String {
    let mut out = format!(
        "{{\n  \"schema_version\": {TRACE_SCHEMA_VERSION},\n  \"entry\": {},\n  \"events\": [",
        quote(entry)
    );
    for (index, event) in events.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("\n    {\n      \"sequence\": ");
        out.push_str(&event.sequence.to_string());
        out.push_str(",\n      \"depth\": ");
        out.push_str(&event.depth.to_string());
        out.push_str(",\n      \"target\": ");
        out.push_str(&quote(&event.target));
        out.push_str(",\n      \"particle_class\": ");
        out.push_str(&quote(&event.particle_class));
        out.push_str(",\n      \"particle\": ");
        out.push_str(&event.particle.to_string());
        out.push_str(",\n      \"answer\": ");
        out.push_str(&event.answer.to_string());
        out.push_str("\n    }");
    }
    if !events.is_empty() {
        out.push('\n');
    }
    out.push_str("  ]\n}");
    out
}

/// One replay case's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Match,
    Mismatch,
    /// Recorded, but not something replay drives: a nested boundary, or one
    /// that belongs to `core` or a module.
    Skipped,
}

impl Outcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Outcome::Match => "match",
            Outcome::Mismatch => "mismatch",
            Outcome::Skipped => "skipped",
        }
    }
}

/// What replaying one recorded boundary produced.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayCase {
    pub sequence: usize,
    pub target: String,
    pub particle_class: String,
    pub outcome: Outcome,
    pub expected: Value,
    /// The answer this run gave, or null for a skipped case.
    pub actual: Value,
    /// Why it was skipped, when it was.
    pub reason: Option<String>,
}

/// Whether a recorded event is one replay drives: the program's own top-level
/// question to itself. Everything else is reachable only in context.
pub fn is_replayable(event: &TraceEvent) -> bool {
    event.depth == 0 && event.target == "this"
}

pub fn render_replay(entry: &str, cases: &[ReplayCase]) -> String {
    let matched = cases
        .iter()
        .filter(|case| case.outcome == Outcome::Match)
        .count();
    let mismatched = cases
        .iter()
        .filter(|case| case.outcome == Outcome::Mismatch)
        .count();
    let skipped = cases
        .iter()
        .filter(|case| case.outcome == Outcome::Skipped)
        .count();
    let mut out = format!(
        "{{\n  \"schema_version\": {TRACE_SCHEMA_VERSION},\n  \"entry\": {},\n  \
         \"replayed\": {},\n  \"matched\": {matched},\n  \"mismatched\": {mismatched},\n  \
         \"skipped\": {skipped},\n  \"cases\": [",
        quote(entry),
        matched + mismatched,
    );
    for (index, case) in cases.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("\n    {\n      \"sequence\": ");
        out.push_str(&case.sequence.to_string());
        out.push_str(",\n      \"target\": ");
        out.push_str(&quote(&case.target));
        out.push_str(",\n      \"particle_class\": ");
        out.push_str(&quote(&case.particle_class));
        out.push_str(",\n      \"status\": ");
        out.push_str(&quote(case.outcome.as_str()));
        out.push_str(",\n      \"expected\": ");
        out.push_str(&case.expected.to_string());
        out.push_str(",\n      \"actual\": ");
        out.push_str(&case.actual.to_string());
        out.push_str(",\n      \"reason\": ");
        match &case.reason {
            Some(reason) => out.push_str(&quote(reason)),
            None => out.push_str("null"),
        }
        out.push_str("\n    }");
    }
    if !cases.is_empty() {
        out.push('\n');
    }
    out.push_str("  ]\n}");
    out
}

/// A trace read back from a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Trace {
    pub entry: String,
    pub events: Vec<TraceEvent>,
}

/// Reads a trace back. Hand-written rather than serde-shaped for the same
/// reason the renderer is, and because a `Value` is already exactly JSON's
/// value space — this is the inverse of `Value`'s `Display`, and it belongs
/// beside the thing it inverts rather than behind a feature flag the browser
/// build turns off.
pub fn parse_trace(text: &str) -> Result<Trace, String> {
    let value = parse_json(text)?;
    let Value::Object(fields) = &value else {
        return Err("a trace is a JSON object".to_string());
    };
    let field = |name: &str| {
        fields
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    match field("schema_version") {
        Some(Value::Number(version)) if version == f64::from(TRACE_SCHEMA_VERSION) => {}
        Some(Value::Number(version)) => {
            return Err(format!(
                "trace schema_version {version} is not {TRACE_SCHEMA_VERSION}"
            ))
        }
        _ => return Err("a trace needs a numeric schema_version".to_string()),
    }
    let entry = match field("entry").as_ref() {
        Some(Value::Str(entry)) => entry.to_string(),
        _ => return Err("a trace needs an 'entry' string".to_string()),
    };
    let Some(events_value) = field("events") else {
        return Err("a trace needs an 'events' array".to_string());
    };
    let Value::Array(items) = &events_value else {
        return Err("a trace needs an 'events' array".to_string());
    };
    let mut events = Vec::new();
    for item in items.iter() {
        let Value::Object(entries) = item else {
            return Err("each trace event is an object".to_string());
        };
        let get = |name: &str| {
            entries
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        let number = |name: &str| match get(name) {
            Some(Value::Number(n)) if n >= 0.0 && n.fract() == 0.0 => Ok(n as usize),
            _ => Err(format!("trace event needs a whole-number '{name}'")),
        };
        let string = |name: &str| match get(name).as_ref() {
            Some(Value::Str(s)) => Ok(s.to_string()),
            _ => Err(format!("trace event needs a string '{name}'")),
        };
        events.push(TraceEvent {
            sequence: number("sequence")?,
            depth: number("depth")?,
            target: string("target")?,
            particle_class: string("particle_class")?,
            particle: get("particle").ok_or("trace event needs a 'particle'")?,
            answer: get("answer").unwrap_or(Value::Null),
        });
    }
    Ok(Trace { entry, events })
}

/// How deep a trace's values may nest before this refuses to read them. A
/// trace is written by this tool, so anything deeper is a malformed or hostile
/// file rather than a program's own data — and refusing is what keeps the
/// parser's recursion off the edge of the stack.
const MAX_JSON_DEPTH: usize = 128;

/// Parses one JSON document into the language's own value space.
pub fn parse_json(text: &str) -> Result<Value, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0usize;
    let value = parse_value(&chars, &mut at, 0)?;
    skip_space(&chars, &mut at);
    if at != chars.len() {
        return Err(format!("unexpected trailing text at character {at}"));
    }
    Ok(value)
}

fn parse_value(chars: &[char], at: &mut usize, depth: usize) -> Result<Value, String> {
    if depth > MAX_JSON_DEPTH {
        return Err("JSON nested deeper than a trace may be".to_string());
    }
    skip_space(chars, at);
    match chars.get(*at) {
        None => Err("unexpected end of JSON".to_string()),
        Some('{') => {
            *at += 1;
            let mut fields = Vec::new();
            skip_space(chars, at);
            if chars.get(*at) == Some(&'}') {
                *at += 1;
                return Ok(Value::Object(std::rc::Rc::new(fields)));
            }
            loop {
                skip_space(chars, at);
                let key = parse_string(chars, at)?;
                skip_space(chars, at);
                if chars.get(*at) != Some(&':') {
                    return Err(format!("expected ':' at character {at}"));
                }
                *at += 1;
                let value = parse_value(chars, at, depth + 1)?;
                fields.push((key, value));
                skip_space(chars, at);
                match chars.get(*at) {
                    Some(',') => *at += 1,
                    Some('}') => {
                        *at += 1;
                        return Ok(Value::Object(std::rc::Rc::new(fields)));
                    }
                    _ => return Err(format!("expected ',' or '}}' at character {at}")),
                }
            }
        }
        Some('[') => {
            *at += 1;
            let mut items = Vec::new();
            skip_space(chars, at);
            if chars.get(*at) == Some(&']') {
                *at += 1;
                return Ok(Value::Array(std::rc::Rc::new(items)));
            }
            loop {
                items.push(parse_value(chars, at, depth + 1)?);
                skip_space(chars, at);
                match chars.get(*at) {
                    Some(',') => *at += 1,
                    Some(']') => {
                        *at += 1;
                        return Ok(Value::Array(std::rc::Rc::new(items)));
                    }
                    _ => return Err(format!("expected ',' or ']' at character {at}")),
                }
            }
        }
        Some('"') => Ok(Value::Str(parse_string(chars, at)?.into())),
        Some('t') => literal(chars, at, "true", Value::Bool(true)),
        Some('f') => literal(chars, at, "false", Value::Bool(false)),
        Some('n') => literal(chars, at, "null", Value::Null),
        Some(_) => parse_number(chars, at),
    }
}

fn literal(chars: &[char], at: &mut usize, word: &str, value: Value) -> Result<Value, String> {
    if chars[*at..].starts_with(&word.chars().collect::<Vec<char>>()[..]) {
        *at += word.chars().count();
        Ok(value)
    } else {
        Err(format!("expected '{word}' at character {at}"))
    }
}

fn parse_number(chars: &[char], at: &mut usize) -> Result<Value, String> {
    let start = *at;
    while let Some(c) = chars.get(*at) {
        if c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E') {
            *at += 1;
        } else {
            break;
        }
    }
    let text: String = chars[start..*at].iter().collect();
    text.parse::<f64>()
        .map(Value::Number)
        .map_err(|_| format!("'{text}' is not a JSON number (at character {start})"))
}

fn parse_string(chars: &[char], at: &mut usize) -> Result<String, String> {
    if chars.get(*at) != Some(&'"') {
        return Err(format!("expected a string at character {at}"));
    }
    *at += 1;
    let mut out = String::new();
    loop {
        match chars.get(*at) {
            None => return Err("unterminated JSON string".to_string()),
            Some('"') => {
                *at += 1;
                return Ok(out);
            }
            Some('\\') => {
                *at += 1;
                let escape = *chars.get(*at).ok_or("unterminated JSON escape")?;
                *at += 1;
                match escape {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'b' => out.push('\u{08}'),
                    'f' => out.push('\u{0c}'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'u' => {
                        let hex: String = chars
                            .get(*at..*at + 4)
                            .ok_or("truncated \\u escape")?
                            .iter()
                            .collect();
                        *at += 4;
                        let code = u32::from_str_radix(&hex, 16)
                            .map_err(|_| format!("'{hex}' is not a \\u escape"))?;
                        out.push(
                            char::from_u32(code).ok_or(format!("'{hex}' is not a character"))?,
                        );
                    }
                    other => return Err(format!("unknown JSON escape '\\{other}'")),
                }
            }
            Some(c) => {
                out.push(*c);
                *at += 1;
            }
        }
    }
}

fn skip_space(chars: &[char], at: &mut usize) {
    while matches!(chars.get(*at), Some(' ' | '\t' | '\n' | '\r')) {
        *at += 1;
    }
}

fn quote(value: &str) -> String {
    let mut quoted = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\u{08}' => quoted.push_str("\\b"),
            '\u{0c}' => quoted.push_str("\\f"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            character if character <= '\u{1f}' => {
                quoted.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{interpreter, lexer, parser};
    use std::rc::Rc;

    fn program(source: &str) -> crate::ast::Program {
        let lexed = lexer::tokenize(source).expect("tokenize");
        parser::parse(&lexed).expect("parse")
    }

    /// The recorder sees real dispatch, in call order, with the nested
    /// boundary marked as nested.
    #[test]
    fn records_emit_boundaries_in_call_order() {
        let source = "Greet { who } =>\n    emit Length { value = who } to core get n\n    \
                      return Greeting { size = n.value }\n\nemit Greet { who = \"abc\" } to this\n";
        let recorder = Rc::new(Recorder::new());
        let mut env = interpreter::Environment::default();
        env.record_trace(Rc::clone(&recorder));
        interpreter::run_with(&program(source), env).expect("run traced program");

        let events = recorder.events();
        assert_eq!(events.len(), 2, "got: {events:?}");
        assert_eq!(events[0].particle_class, "Greet");
        assert_eq!(events[0].target, "this");
        assert_eq!(events[0].depth, 0);
        assert_eq!(
            events[0].answer.to_string(),
            "{\"_class\":\"Greeting\",\"size\":3}"
        );
        assert_eq!(events[1].particle_class, "Length");
        assert_eq!(events[1].target, "core");
        assert_eq!(events[1].depth, 1, "a nested boundary records its depth");
        assert!(is_replayable(&events[0]));
        assert!(!is_replayable(&events[1]));
    }

    /// A run with no recorder attached records nothing and is the ordinary
    /// path every program already takes.
    #[test]
    fn tracing_is_opt_in() {
        let source = "Greet {} =>\n    return Ack {}\n\nemit Greet {} to this\n";
        let env = interpreter::Environment::default();
        interpreter::run_with(&program(source), env).expect("run untraced program");
    }

    /// Rendering then reading a trace gives the same events back — that is
    /// what makes the file a fixture rather than a report.
    #[test]
    fn a_rendered_trace_reads_back_unchanged() {
        let source = "Greet { who } =>\n    return Greeting { text = who }\n\n\
                      emit Greet { who = \"a\\\"b\\nc\" } to this\n";
        let recorder = Rc::new(Recorder::new());
        let mut env = interpreter::Environment::default();
        env.record_trace(Rc::clone(&recorder));
        interpreter::run_with(&program(source), env).expect("run traced program");

        let events = recorder.events();
        let text = render_trace("main.code", &events);
        let read = parse_trace(&text).expect("read the trace back");
        assert_eq!(read.entry, "main.code");
        assert_eq!(read.events, events);
    }

    #[test]
    fn refuses_a_trace_from_another_schema_version() {
        let text = "{\"schema_version\": 2, \"entry\": \"main.code\", \"events\": []}";
        assert!(parse_trace(text).is_err());
    }

    #[test]
    fn parses_every_json_value_kind() {
        let value = parse_json("{\"a\": [1, -2.5, 1e2, true, false, null, \"\\u00e9\"]}")
            .expect("parse JSON");
        assert_eq!(
            value.to_string(),
            "{\"a\":[1,-2.5,100,true,false,null,\"é\"]}"
        );
    }
}

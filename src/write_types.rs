//! `code check --write-types`: writes the label the inference found into
//! every handler field that has none (ticket 122, A.3).
//!
//! The owner's rule is that a handler's field list says what arrives, to the
//! last field. 1,567 handlers across the apps had 2,486 fields without a
//! label; this writes what the program itself shows — what is sent to each
//! handler, and what flows on from there (`infer.rs`) — and lists what it
//! cannot decide (nothing is ever sent, or only null) for a person to say.
//!
//! Only the project's own files are written: a file outside the directory
//! the program was found in, or under `.code/` (installed genes and
//! modules), is someone else's.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::ast::{Label, Program};
use crate::infer::{suggested_labels, UNKNOWN};

/// What one run did.
#[derive(Default)]
pub struct Report {
    /// Labels written, per file.
    pub written: BTreeMap<String, usize>,
    /// Fields left without a label: `file:line Handler.field`.
    pub undecided: Vec<String>,
}

/// A label longer than this is written over several lines, and the
/// handler's field list with it, one field per line.
const INLINE: usize = 60;

/// What the fixtures saw arrive (`CODE_OBSERVE`): one merged label per
/// handler class and field.
pub fn read_observed(text: &str) -> HashMap<(String, String), Label> {
    let mut seen: HashMap<(String, String), Label> = HashMap::new();
    for line in text.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(class), Some(field), Some(label)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let Ok(label) = crate::parser::parse_label(label) else {
            continue;
        };
        let key = (class.to_string(), field.to_string());
        let merged = match seen.remove(&key) {
            Some(earlier) => earlier.merge(label),
            None => label,
        };
        seen.insert(key, merged);
    }
    seen
}

/// `observed` fills in the fields the program itself does not decide — the
/// handlers only the host, a module or another app sends to.
pub fn write_types(
    program: &Program,
    project: &Path,
    observed: &HashMap<(String, String), Label>,
) -> Result<Report, String> {
    let mut report = Report::default();
    // Per file: its text and the edits to make in it.
    let mut files: BTreeMap<PathBuf, (String, Vec<Edit>)> = BTreeMap::new();
    for suggestion in suggested_labels(program) {
        let (Some(origin), Some(span)) = (&suggestion.origin, suggestion.span) else {
            continue;
        };
        let path = absolute(&origin.file);
        if !owned(&path, project) {
            continue;
        }
        let entry = files
            .entry(path)
            .or_insert_with(|| (origin.source.clone(), Vec::new()));
        let seen = observed.get(&(suggestion.handler.clone(), suggestion.field.clone()));
        let label = match suggestion.label {
            Some(label) => fill(label, seen),
            None => seen.cloned(),
        };
        match label {
            Some(label) => entry.1.push(Edit {
                start: span.start as usize,
                field: suggestion.field,
                label,
            }),
            None => {
                let line = line_of(&origin.source, span.start as usize);
                report.undecided.push(format!(
                    "{}:{line} {}.{}",
                    origin.file, suggestion.handler, suggestion.field
                ));
            }
        }
    }
    for (path, (source, edits)) in files {
        if edits.is_empty() {
            continue;
        }
        let count = edits.len();
        let relabelled = relabel(&source, edits);
        // Only the headers it labels change: the rest of the file stays as
        // its author laid it out, so the diff is the labels and nothing else.
        // It must still read, or nothing is written.
        let reads = crate::lexer::tokenize(&relabelled)
            .and_then(|lexed| crate::parser::parse(&lexed).map(|_| ()));
        if let Err(e) = reads {
            return Err(format!(
                "{}: the labelled file would not parse ({}) — nothing written",
                path.display(),
                e.msg
            ));
        }
        std::fs::write(&path, relabelled)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        report.written.insert(path.display().to_string(), count);
    }
    Ok(report)
}

/// What the program shows (`program`), completed by what the fixtures saw
/// (`seen`): a part the program leaves undecided takes what was seen, and an
/// object takes every field either knows — the program names the fields it
/// reads, the fixtures the ones that were really there. `None` when neither
/// says anything; a part still undecided below the top is `Any`.
fn fill(program: Label, seen: Option<&Label>) -> Option<Label> {
    let any = || Label::Named("Any".to_string());
    match program {
        Label::Named(name) if name == UNKNOWN => seen.cloned(),
        Label::Fields(fields) => {
            let seen_fields = match seen {
                Some(Label::Fields(seen_fields)) => seen_fields.clone(),
                _ => Vec::new(),
            };
            let mut out: Vec<(String, Label)> = fields
                .into_iter()
                .map(|(name, label)| {
                    let seen_field = seen_fields.iter().find(|(n, _)| *n == name).map(|(_, l)| l);
                    let label = fill(label, seen_field).unwrap_or_else(any);
                    (name, label)
                })
                .collect();
            for (name, label) in seen_fields {
                if !out.iter().any(|(n, _)| *n == name) {
                    out.push((name, label));
                }
            }
            Some(Label::Fields(out))
        }
        Label::ArrayOf(element) => {
            let seen_element = match seen {
                Some(Label::ArrayOf(inner)) => Some(&**inner),
                _ => None,
            };
            Some(Label::ArrayOf(Box::new(
                fill(*element, seen_element).unwrap_or_else(any),
            )))
        }
        Label::ObjectOf(values) => {
            let seen_values = match seen {
                Some(Label::ObjectOf(inner)) => Some(&**inner),
                _ => None,
            };
            Some(Label::ObjectOf(Box::new(
                fill(*values, seen_values).unwrap_or_else(any),
            )))
        }
        other => Some(other),
    }
}

struct Edit {
    /// Where the field's declaration starts, in characters.
    start: usize,
    field: String,
    label: Label,
}

fn absolute(file: &str) -> PathBuf {
    let path = PathBuf::from(file);
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    path.canonicalize().unwrap_or(path)
}

fn owned(path: &Path, project: &Path) -> bool {
    let project = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf());
    path.starts_with(&project) && !path.components().any(|c| c.as_os_str() == ".code")
}

fn line_of(source: &str, at: usize) -> usize {
    source.chars().take(at).filter(|c| *c == '\n').count() + 1
}

/// `source` with each edit's label written after its field's name. A long
/// label goes over several lines, and then its handler's whole field list is
/// laid out one field per line.
fn relabel(source: &str, mut edits: Vec<Edit>) -> String {
    let chars: Vec<char> = source.chars().collect();
    edits.sort_by_key(|edit| edit.start);
    // Group the edits by the field list they are in: the `{` before a
    // field's declaration opens it.
    let mut out = String::new();
    let mut at = 0;
    let mut i = 0;
    while i < edits.len() {
        let open = list_start(&chars, edits[i].start);
        let mut group = vec![i];
        while i + 1 < edits.len() && list_start(&chars, edits[i + 1].start) == open {
            i += 1;
            group.push(i);
        }
        let long = group
            .iter()
            .any(|&g| edits[g].label.spelling().chars().count() > INLINE);
        if !long {
            for &g in &group {
                let after_name = edits[g].start + edits[g].field.chars().count();
                out.extend(&chars[at..after_name]);
                out.push_str(" ∈ ");
                out.push_str(&edits[g].label.spelling());
                at = after_name;
            }
        } else {
            // The whole list, one field per line: every declaration in it,
            // labelled or not, kept as written apart from the new labels.
            let close = matching_close(&chars, open);
            out.extend(&chars[at..=open]);
            let fields = declarations(&chars, open, close);
            for (start, end) in fields {
                out.push_str("\n    ");
                match group.iter().find(|&&g| edits[g].start == start) {
                    Some(&g) => {
                        let after_name = start + edits[g].field.chars().count();
                        out.extend(&chars[start..after_name]);
                        out.push_str(" ∈ ");
                        out.push_str(&pretty(&edits[g].label, 1));
                        out.extend(&chars[after_name..end]);
                    }
                    None => out.extend(&chars[start..end]),
                }
            }
            out.push('\n');
            at = close;
        }
        i += 1;
    }
    out.extend(&chars[at..]);
    out
}

/// The `{` opening the field list a declaration at `start` is in — the
/// first unmatched one before it, stepping over earlier fields' nested
/// labels.
fn list_start(chars: &[char], start: usize) -> usize {
    let mut depth = 0;
    for k in (0..start).rev() {
        match chars[k] {
            '}' => depth += 1,
            '{' if depth == 0 => return k,
            '{' => depth -= 1,
            _ => {}
        }
    }
    start
}

/// The `}` closing the `{` at `open`.
fn matching_close(chars: &[char], open: usize) -> usize {
    let mut depth = 0;
    for (k, c) in chars.iter().enumerate().skip(open) {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return k;
                }
            }
            _ => {}
        }
    }
    chars.len()
}

/// The declarations between `{` and `}` of a field list, split at the commas
/// and line breaks that are not inside a nested label.
fn declarations(chars: &[char], open: usize, close: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut start = None;
    for k in open + 1..close {
        let c = chars[k];
        match c {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            _ => {}
        }
        let separator = depth == 0 && (c == ',' || c == '\n');
        if separator {
            if let Some(s) = start.take() {
                out.push((s, trim_end(chars, s, k)));
            }
        } else if start.is_none() && !c.is_whitespace() {
            start = Some(k);
        }
    }
    if let Some(s) = start {
        out.push((s, trim_end(chars, s, close)));
    }
    out
}

fn trim_end(chars: &[char], start: usize, mut end: usize) -> usize {
    while end > start && chars[end - 1].is_whitespace() {
        end -= 1;
    }
    end
}

/// A label over several lines, nested objects indented under their field.
fn pretty(label: &Label, depth: usize) -> String {
    let spelled = label.spelling();
    if spelled.chars().count() <= INLINE {
        return spelled;
    }
    match label {
        Label::Fields(fields) => {
            let indent = "    ".repeat(depth + 1);
            let mut out = String::from("{");
            for (name, field) in fields {
                out.push('\n');
                out.push_str(&indent);
                out.push_str(&crate::ast::field_name(name));
                out.push_str(" ∈ ");
                out.push_str(&pretty(field, depth + 1));
            }
            out.push('\n');
            out.push_str(&"    ".repeat(depth));
            out.push('}');
            out
        }
        Label::ArrayOf(inner) => format!("Array of {}", pretty(inner, depth)),
        Label::ObjectOf(inner) => format!("Object of {}", pretty(inner, depth)),
        Label::Named(_) | Label::Open(_) | Label::Union(_) | Label::Value(_) => spelled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer, parser};

    fn labelled(source: &str) -> String {
        let lexed = lexer::tokenize(source).expect("tokenize");
        let program = parser::parse(&lexed).expect("parse");
        let mut edits = Vec::new();
        for suggestion in suggested_labels(&program) {
            if let (Some(span), Some(label)) = (
                suggestion.span,
                suggestion.label.and_then(|l| fill(l, None)),
            ) {
                edits.push(Edit {
                    start: span.start as usize,
                    field: suggestion.field,
                    label,
                });
            }
        }
        crate::format::format(&relabel(source, edits)).expect("still parses")
    }

    #[test]
    fn writes_what_is_sent_into_the_field_list() {
        let out = labelled(
            "Move { fish, by ∈ Number } =>\n    return Moved {}\n\nemit Move { fish = { x = 1, y = 2 }, by = 1 } to this\n",
        );
        assert!(
            out.starts_with("Move { fish ∈ { x ∈ Number, y ∈ Number }, by ∈ Number } =>"),
            "{out}"
        );
    }

    #[test]
    fn a_long_label_lays_the_list_out_one_field_per_line() {
        let out = labelled(
            "Draw { scene, title as heading } =>\n    return Drawn {}\n\nemit Draw { scene = { shapes = [{ op = \"dot\", x = 1, y = 2, colour = \"red\" }], width = 10, height = 20 }, title = \"t\" } to this\n",
        );
        assert!(out.contains("Draw {\n    scene ∈ {\n"), "{out}");
        assert!(out.contains("    title ∈ String as heading\n} =>"), "{out}");
        // and what it wrote reads back as the same labels
        let lexed = lexer::tokenize(&out).unwrap();
        parser::parse(&lexed).unwrap();
    }

    #[test]
    fn a_field_nothing_is_sent_to_is_left_alone() {
        let out = labelled("Hello { name } =>\n    return Hi {}\n");
        assert!(out.starts_with("Hello { name } =>"), "{out}");
    }

    #[test]
    fn a_name_that_is_not_a_word_is_quoted() {
        let out = labelled(
            "Tag { attrs } =>\n    return Tagged {}\n\nemit Tag { attrs = { \"aria-label\" = \"x\", \"\\$ref\" = 1, \"to\" = true } } to this\n",
        );
        assert!(
            out.starts_with(
                "Tag { attrs ∈ { \"\\$ref\" ∈ Number, \"aria-label\" ∈ String, \"to\" ∈ Boolean } } =>"
            ),
            "{out}"
        );
    }

    #[test]
    fn what_the_fixtures_saw_completes_what_the_program_shows() {
        let program = crate::parser::parse_label("Array of { id ∈ _, status ∈ String }").unwrap();
        let seen = crate::parser::parse_label(
            "Array of { id ∈ Number, status ∈ String, subject ∈ String }",
        )
        .unwrap();
        assert_eq!(
            fill(program, Some(&seen)).unwrap().spelling(),
            "Array of { id ∈ Number, status ∈ String, subject ∈ String }"
        );
        let observed = read_observed(
            "Log\tmessage\tString\nLog\tmessage\tString\nLog\tlevel\tNumber\nLog\tlevel\tString\n",
        );
        assert_eq!(
            observed[&("Log".to_string(), "message".to_string())].spelling(),
            "String"
        );
        assert_eq!(
            observed[&("Log".to_string(), "level".to_string())].spelling(),
            "Any"
        );
    }

    #[test]
    fn kinds_that_clash_are_any() {
        let out = labelled(
            "Show { value } =>\n    return Shown {}\n\nemit Show { value = 1 } to this\nemit Show { value = \"one\" } to this\n",
        );
        assert!(out.starts_with("Show { value ∈ Any } =>"), "{out}");
    }
}

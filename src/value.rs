use std::fmt;
use std::rc::Rc;

/// A runtime value. The language's value model is exactly JSON's six kinds —
/// no value can exist outside this set, and there are no type keywords to
/// name or extend it. Variables are untyped bindings — a name can point to
/// any `Value` at any time; this enum is the thing that's typed, never the
/// binding.
///
/// `Str`/`Array`/`Object` wrap their heap data in `Rc` so that cloning a
/// `Value` — which happens on every variable read (`Environment::get`) and
/// every time one value is embedded in another (`arr = [x]`) — is O(1)
/// regardless of size, instead of a deep copy. This is safe with zero extra
/// bookkeeping only because nothing in the language can mutate a value
/// in place yet (see memory `new-code-language-design`); adding array/object
/// mutation later will need its own copy-on-write decision.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Str(Rc<str>),
    Bool(bool),
    Null,
    Array(Rc<Vec<Value>>),
    /// Insertion-ordered `key: value` pairs. JSON doesn't mandate order, but
    /// preserving it makes output deterministic and matches what a person
    /// wrote — a plain `Vec` is fine at this scale (linear key lookup).
    Object(Rc<Vec<(String, Value)>>),
}

impl fmt::Display for Value {
    /// Renders as JSON text — this doubles as the language's serialization
    /// format for free, since every value already lives in JSON's value
    /// space.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(n) => write!(f, "{n}"),
            Value::Str(s) => write_json_string(f, s),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Null => write!(f, "null"),
            Value::Array(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "]")
            }
            Value::Object(fields) => {
                write!(f, "{{")?;
                for (i, (key, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write_json_string(f, key)?;
                    write!(f, ":{value}")?;
                }
                write!(f, "}}")
            }
        }
    }
}

fn write_json_string(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    write!(f, "\"")?;
    for c in s.chars() {
        match c {
            '"' => write!(f, "\\\"")?,
            '\\' => write!(f, "\\\\")?,
            '\n' => write!(f, "\\n")?,
            '\t' => write!(f, "\\t")?,
            c => write!(f, "{c}")?,
        }
    }
    write!(f, "\"")
}

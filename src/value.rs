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
/// Every traversal below — `Drop`, `PartialEq`, `Display` — is written with
/// an explicit work stack rather than recursion, and `runtime.c` does the
/// same for its three equivalents. Nesting depth is bounded only by a loop's
/// iteration count (`loop x over xs { a = [a] }`), not by how many brackets
/// someone typed, so one native stack frame per level overflows at around
/// 16k deep — see `tests/deep_nesting.code`.
#[derive(Debug, Clone)]
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

/// Dropping a nested `Value` would otherwise recurse once per level: the
/// derived `Drop` releases an `Rc<Vec<Value>>`, whose `Vec` drops each
/// element, each of which repeats. This unrolls that into a work stack —
/// `Rc::get_mut` succeeds exactly when this is the last reference and the
/// contents are about to be dropped anyway, so emptying the container first
/// leaves nothing for the automatic drop to recurse into.
impl Drop for Value {
    fn drop(&mut self) {
        let mut stack = Vec::new();
        take_children(self, &mut stack);
        while let Some(mut value) = stack.pop() {
            take_children(&mut value, &mut stack);
            // `value` is dropped here, re-entering this impl — but it has
            // just been emptied, so that drop bottoms out immediately.
        }
    }
}

fn take_children(value: &mut Value, stack: &mut Vec<Value>) {
    match value {
        Value::Array(items) => {
            if let Some(items) = Rc::get_mut(items) {
                stack.append(items);
            }
        }
        Value::Object(fields) => {
            if let Some(fields) = Rc::get_mut(fields) {
                stack.extend(fields.drain(..).map(|(_, value)| value));
            }
        }
        _ => {}
    }
}

/// Deep structural equality — including that objects compare *positionally*
/// (same keys in the same order), not as sets of pairs. Well-defined for any
/// two values: mismatched kinds are simply unequal, never an error.
/// `runtime.c`'s `code_values_equal` must match this exactly.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        let mut pairs = vec![(self, other)];
        while let Some((a, b)) = pairs.pop() {
            let equal = match (a, b) {
                (Value::Number(a), Value::Number(b)) => a == b,
                (Value::Str(a), Value::Str(b)) => a == b,
                (Value::Bool(a), Value::Bool(b)) => a == b,
                (Value::Null, Value::Null) => true,
                (Value::Array(a), Value::Array(b)) => {
                    if a.len() != b.len() {
                        return false;
                    }
                    pairs.extend(a.iter().zip(b.iter()));
                    true
                }
                (Value::Object(a), Value::Object(b)) => {
                    if a.len() != b.len() {
                        return false;
                    }
                    for ((a_key, a_value), (b_key, b_value)) in a.iter().zip(b.iter()) {
                        if a_key != b_key {
                            return false;
                        }
                        pairs.push((a_value, b_value));
                    }
                    true
                }
                _ => false,
            };
            if !equal {
                return false;
            }
        }
        true
    }
}

/// One step of `Display`'s traversal. Closers and separators are pushed onto
/// the same stack as the values they follow, which is what removes the need
/// for a recursive call to "come back" and finish a container.
enum Step<'a> {
    Value(&'a Value),
    /// A `,`, `]` or `}` to emit verbatim.
    Punct(&'static str),
    /// An object key, emitted quoted and followed by `:`.
    Key(&'a str),
}

impl fmt::Display for Value {
    /// Renders as JSON text — this doubles as the language's serialization
    /// format for free, since every value already lives in JSON's value
    /// space.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut stack = vec![Step::Value(self)];
        while let Some(step) = stack.pop() {
            match step {
                Step::Punct(s) => f.write_str(s)?,
                Step::Key(key) => {
                    write_json_string(f, key)?;
                    f.write_str(":")?;
                }
                Step::Value(Value::Number(n)) => write!(f, "{n}")?,
                Step::Value(Value::Str(s)) => write_json_string(f, s)?,
                Step::Value(Value::Bool(b)) => write!(f, "{b}")?,
                Step::Value(Value::Null) => f.write_str("null")?,
                // Pushed in reverse so they pop in source order, with the
                // closing bracket pushed first and therefore popped last.
                Step::Value(Value::Array(items)) => {
                    f.write_str("[")?;
                    stack.push(Step::Punct("]"));
                    for (i, item) in items.iter().enumerate().rev() {
                        stack.push(Step::Value(item));
                        if i > 0 {
                            stack.push(Step::Punct(","));
                        }
                    }
                }
                Step::Value(Value::Object(fields)) => {
                    f.write_str("{")?;
                    stack.push(Step::Punct("}"));
                    for (i, (key, value)) in fields.iter().enumerate().rev() {
                        stack.push(Step::Value(value));
                        stack.push(Step::Key(key));
                        if i > 0 {
                            stack.push(Step::Punct(","));
                        }
                    }
                }
            }
        }
        Ok(())
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

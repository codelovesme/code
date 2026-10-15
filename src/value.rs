use std::fmt;
use std::rc::Rc;

/// Shared UTF-8 text with spare capacity for an exclusively held value.
/// Cloning shares the bytes; writes detach before changing a shared string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text(Rc<String>);

impl Text {
    pub(crate) fn push_str(&mut self, suffix: &str) {
        Rc::make_mut(&mut self.0).push_str(suffix);
    }
}

impl std::ops::Deref for Text {
    type Target = str;
    fn deref(&self) -> &str {
        self.0.as_str()
    }
}

impl AsRef<str> for Text {
    fn as_ref(&self) -> &str {
        self
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self)
    }
}

impl From<String> for Text {
    fn from(value: String) -> Self {
        Self(Rc::new(value))
    }
}

impl From<&str> for Text {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}

/// A runtime value. The language's value model is exactly JSON's six kinds —
/// no value can exist outside this set, and there are no type keywords to
/// name or extend it. Variables are untyped bindings — a name can point to
/// any `Value` at any time; this enum is the thing that's typed, never the
/// binding.
///
/// `Str`/`Array`/`Object` wrap their heap data in `Rc`. Read-only operands
/// borrow their values; a value that is stored or handed on is cloned,
/// retaining the heap block in O(1) regardless of its size. A part write
/// uses `Rc::make_mut`, so another holder continues to see its old value.
///
/// Every traversal below — `Drop`, `PartialEq`, `Display` — is written with
/// an explicit work stack rather than recursion, and `runtime.c` does the
/// same for its three equivalents. Nesting depth is bounded only by a loop's
/// iteration count (`loop x over xs { a = [a] }`), not by how many brackets
/// someone typed, so one native stack frame per level overflows at around
/// 16k deep — see `tests/stress_deep_nesting.code`.
#[derive(Debug, Clone)]
pub enum Value {
    Number(f64),
    Str(Text),
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
    #[inline]
    fn drop(&mut self) {
        // A number, a text, a flag or null has no children to unroll, and
        // neither does a container someone else still holds.
        let sole = match self {
            Value::Array(items) => Rc::strong_count(items) == 1,
            Value::Object(fields) => Rc::strong_count(fields) == 1,
            _ => false,
        };
        if sole {
            drop_children(self);
        }
    }
}

#[inline(never)]
fn drop_children(value: &mut Value) {
    {
        let mut stack = Vec::new();
        take_children(value, &mut stack);
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

/// Deep structural equality. Well-defined for any two values: mismatched
/// kinds are simply unequal, never an error. `runtime.c`'s
/// `code_values_equal` must match this exactly.
///
/// **Objects compare by field name, not by position.** JSON gives an
/// object's members no order, so neither does equality here:
/// `{ a = 1, b = 2 }` and `{ b = 2, a = 1 }` are the same value. The
/// language still *keeps* the order a field was written in — that is what
/// iteration and printing show, and what a merge's "right value, left
/// position" rule is about — but keeping an order and comparing by it are
/// two different decisions, and only the first one was ever wanted.
///
/// This compared positionally until 2026-09-09, which was not a decision so
/// much as the shape of the representation showing through: an object is a
/// list of pairs, and comparing the lists compared their order too. It cost
/// real time — a module rebuilding a result from JSON hands the fields back
/// in whatever order its parser chose, and an otherwise correct
/// `assert v = { a, b }` failed on nothing.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        // Scalars have no children to walk. In particular, comparing loop
        // counters must not allocate a traversal stack just to read a number.
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => return a == b,
            (Value::Str(a), Value::Str(b)) => return a == b,
            (Value::Bool(a), Value::Bool(b)) => return a == b,
            (Value::Null, Value::Null) => return true,
            (Value::Array(_), Value::Array(_)) | (Value::Object(_), Value::Object(_)) => {}
            _ => return false,
        }
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
                    // A name can appear twice — a computed key (`{ "$k" = v }`)
                    // may collide with a literal one, and nothing rejects
                    // that — so repeats of a name are paired in the order
                    // they appear rather than searched for by value. That
                    // keeps the pairing injective, which with equal lengths
                    // makes it a bijection, and keeps this a work-stack walk:
                    // searching by value would mean comparing values inside
                    // the matching loop, which is the recursion the stack
                    // exists to avoid.
                    for (i, (a_key, a_value)) in a.iter().enumerate() {
                        let seen = a[..i].iter().filter(|(k, _)| k == a_key).count();
                        let Some((_, b_value)) = b.iter().filter(|(k, _)| k == a_key).nth(seen)
                        else {
                            return false;
                        };
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

/// The label a value fits, to the last field — what `CODE_OBSERVE` records
/// for each particle a handler receives (ticket 122, A.3). `None` for null:
/// it fits every label and says nothing. An object with many keys, or keys
/// that are not names, is taken for a map.
pub fn label_of_value(value: &Value) -> Option<crate::ast::Label> {
    use crate::ast::Label;
    let any = || Label::Named("Any".to_string());
    match value {
        Value::Null => None,
        Value::Number(_) => Some(Label::Named("Number".to_string())),
        Value::Str(_) => Some(Label::Named("String".to_string())),
        Value::Bool(_) => Some(Label::Named("Boolean".to_string())),
        Value::Array(items) => Some(Label::ArrayOf(Box::new(
            items
                .iter()
                .filter_map(label_of_value)
                .reduce(Label::merge)
                .unwrap_or_else(any),
        ))),
        Value::Object(fields) => {
            let named = |key: &str| {
                key.chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                    && key.chars().all(|c| c.is_alphanumeric() || c == '_')
            };
            let fields: Vec<&(String, Value)> =
                fields.iter().filter(|(key, _)| key != "_class").collect();
            if fields.len() > 24 || !fields.iter().all(|(key, _)| named(key)) {
                return Some(Label::ObjectOf(Box::new(
                    fields
                        .iter()
                        .filter_map(|(_, v)| label_of_value(v))
                        .reduce(Label::merge)
                        .unwrap_or_else(any),
                )));
            }
            Some(Label::Fields(
                fields
                    .iter()
                    .map(|(key, v)| (key.clone(), label_of_value(v).unwrap_or_else(any)))
                    .collect(),
            ))
        }
    }
}

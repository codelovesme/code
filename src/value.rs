use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::rc::{Rc, Weak};

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
    Str(Rc<str>),
    Bool(bool),
    Null,
    Array(Rc<Vec<Value>>),
    /// Insertion-ordered `key: value` pairs. JSON doesn't mandate order, but
    /// preserving it makes output deterministic and matches what a person
    /// wrote — a plain `Vec` is fine at this scale (linear key lookup).
    Object(Rc<Vec<(String, Value)>>),
}

// Keep the public Rc<Vec<...>> value shape. A weak owner identifies a cached
// immutable vector without keeping its values alive. Rc::make_mut dissociates
// weak owners before mutation; interpreter writes also drop the cache first.
// Entries are bounded and do not contain references into the vector.
type ObjectFields = Rc<Vec<(String, Value)>>;
struct ObjectIndex {
    owner: Weak<Vec<(String, Value)>>,
    positions: Box<[usize]>,
}
struct ObjectIndexes {
    entries: VecDeque<ObjectIndex>,
    check: bool,
}
thread_local! {
    static OBJECT_INDEXES: RefCell<ObjectIndexes> = RefCell::new(ObjectIndexes {
        entries: VecDeque::new(),
        check: std::env::var_os("CODE_OBJECT_INDEX_CHECK").is_some(),
    });
}
fn object_key_hash(key: &str) -> usize {
    key.bytes().fold(2166136261u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16777619)
    }) as usize
}
fn scan_object(fields: &[(String, Value)], key: &str) -> Option<usize> {
    fields.iter().position(|(name, _)| name == key)
}

pub(crate) fn object_position(fields: &ObjectFields, key: &str) -> Option<usize> {
    if fields.len() <= 16 {
        return scan_object(fields, key);
    }
    OBJECT_INDEXES
        .try_with(|cache| {
            let mut cache = cache.borrow_mut();
            let owner = Rc::as_ptr(fields);
            let at = match cache
                .entries
                .iter()
                .position(|entry| entry.owner.as_ptr() == owner)
            {
                Some(at) => at,
                None => {
                    cache.entries.retain(|entry| entry.owner.strong_count() > 0);
                    if cache.entries.len() == 32 {
                        cache.entries.pop_front();
                    }
                    let mut positions = vec![usize::MAX; (fields.len() * 2).next_power_of_two()];
                    let mask = positions.len() - 1;
                    for (i, (key, _)) in fields.iter().enumerate() {
                        let mut bucket = object_key_hash(key) & mask;
                        while positions[bucket] != usize::MAX {
                            if fields[positions[bucket]].0 == *key {
                                break;
                            }
                            bucket = (bucket + 1) & mask;
                        }
                        if positions[bucket] == usize::MAX {
                            positions[bucket] = i;
                        }
                    }
                    cache.entries.push_back(ObjectIndex {
                        owner: Rc::downgrade(fields),
                        positions: positions.into_boxed_slice(),
                    });
                    cache.entries.len() - 1
                }
            };
            let index = &cache.entries[at];
            let mask = index.positions.len() - 1;
            let mut bucket = object_key_hash(key) & mask;
            let found = loop {
                let at = index.positions[bucket];
                if at == usize::MAX {
                    break None;
                }
                if fields[at].0 == key {
                    break Some(at);
                }
                bucket = (bucket + 1) & mask;
            };
            if cache.check {
                assert_eq!(
                    found,
                    scan_object(fields, key),
                    "object index differs from scan"
                );
            }
            found
        })
        .unwrap_or_else(|_| scan_object(fields, key))
}

pub(crate) fn object_field<'a>(fields: &'a ObjectFields, key: &str) -> Option<&'a Value> {
    object_position(fields, key).map(|at| &fields[at].1)
}

pub(crate) fn forget_object_index(fields: &ObjectFields) {
    if fields.len() <= 16 {
        return;
    }
    // TLS may already be gone if an embedding drops values at thread exit.
    let _ = OBJECT_INDEXES.try_with(|cache| {
        let owner = Rc::as_ptr(fields);
        cache
            .borrow_mut()
            .entries
            .retain(|entry| entry.owner.as_ptr() != owner);
    });
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
            // Remove our weak owner so get_mut can still unroll the drop.
            if Rc::strong_count(fields) == 1 {
                forget_object_index(fields);
            }
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

#[cfg(test)]
mod object_index_tests {
    use super::*;

    fn large_fields() -> ObjectFields {
        Rc::new(
            (0..40)
                .map(|i| (format!("k{i}"), Value::Number(i as f64)))
                .collect(),
        )
    }

    #[test]
    fn cached_positions_follow_copies_growth_and_external_make_mut() {
        let mut fields = large_fields();
        assert_eq!(object_position(&fields, "k20"), Some(20));
        // A safe Rust caller may mutate without our explicit invalidation.
        Rc::make_mut(&mut fields).swap(0, 20);
        assert_eq!(object_position(&fields, "k20"), Some(0));
        let saved = Rc::clone(&fields);
        forget_object_index(&fields);
        Rc::make_mut(&mut fields).push(("new".to_string(), Value::Number(99.0)));
        assert_eq!(object_field(&fields, "new"), Some(&Value::Number(99.0)));
        assert_eq!(object_field(&saved, "new"), None);
        assert_eq!(object_field(&saved, "k20"), Some(&Value::Number(20.0)));
        drop(Value::Object(fields));
        drop(Value::Object(saved));
    }

    #[test]
    fn duplicate_keys_and_hash_collisions_keep_the_first_match() {
        let mut fields = large_fields();
        Rc::make_mut(&mut fields).extend([
            ("costarring".to_string(), Value::Number(7.0)),
            ("liquid".to_string(), Value::Number(9.0)),
            ("k20".to_string(), Value::Number(999.0)),
        ]);
        assert_eq!(object_key_hash("costarring"), object_key_hash("liquid"));
        assert_eq!(
            object_field(&fields, "costarring"),
            Some(&Value::Number(7.0))
        );
        assert_eq!(object_field(&fields, "liquid"), Some(&Value::Number(9.0)));
        assert_eq!(object_field(&fields, "k20"), Some(&Value::Number(20.0)));
        assert_eq!(object_field(&fields, "absent"), None);
        drop(Value::Object(fields));
    }

    #[test]
    fn cached_weak_owners_do_not_turn_iterative_drop_into_recursion() {
        let mut value = Value::Null;
        for _ in 0..5000 {
            let mut fields: Vec<_> = (0..17)
                .map(|i| (format!("k{i}"), Value::Number(i as f64)))
                .collect();
            fields.push(("child".to_string(), value));
            let fields = Rc::new(fields);
            assert_eq!(object_position(&fields, "child"), Some(17));
            value = Value::Object(fields);
        }
        drop(value);
    }
}

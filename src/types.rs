//! Data types as sets of values (ticket 123).
//!
//! A label (`ast::Label`) names a set of values: one of the six types, `Any`,
//! an object with these fields, a list of these, one of several (`∪`), or
//! just one value (`"red"`). `type Name = <label>` gives one a name, so a
//! label can say `Color`, and a type can contain itself (`Element`'s
//! children are elements). `Particle` is predefined: the objects with a
//! `_class` field. A capitalised name no `type` declares is the particles of
//! that class, which is what `r ∈ Exception` has always meant.
//!
//! The names are the whole program's: a gene's `type` is its app's too.

use std::collections::{HashMap, HashSet};

use crate::ast::{Label, LabelValue, Program, Stmt, ValueKind};

/// Every `type` the program declares, by name, with the predefined ones.
#[derive(Debug, Clone, Default)]
pub struct Types {
    defs: HashMap<String, Label>,
}

impl Types {
    /// The types `program` declares, its linked files' included. A name
    /// declared twice is the first one's (`duplicates` finds them).
    pub fn of(program: &Program) -> Types {
        let mut types = Types::predefined();
        types.collect(&program.statements);
        types
    }

    /// Only the predefined types.
    pub fn predefined() -> Types {
        let mut defs = HashMap::new();
        defs.insert(
            "Particle".to_string(),
            Label::Open(vec![(
                "_class".to_string(),
                Label::Named("String".to_string()),
            )]),
        );
        Types { defs }
    }

    fn collect(&mut self, statements: &[Stmt]) {
        for statement in statements {
            match statement {
                Stmt::TypeDef { name, label } => {
                    self.defs
                        .entry(name.clone())
                        .or_insert_with(|| label.clone());
                }
                Stmt::Import { body, .. } => self.collect(body),
                _ => {}
            }
        }
    }

    /// The names declared more than once in `statements` and the files they
    /// link — each a mistake `code check` reports.
    pub fn duplicates(program: &Program) -> Vec<String> {
        fn walk(statements: &[Stmt], seen: &mut HashSet<String>, twice: &mut Vec<String>) {
            for statement in statements {
                match statement {
                    Stmt::TypeDef { name, .. } => {
                        let predefined = Types::predefined().defs.contains_key(name);
                        if (predefined || !seen.insert(name.clone())) && !twice.contains(name) {
                            twice.push(name.clone());
                        }
                    }
                    Stmt::Import { body, .. } => walk(body, seen, twice),
                    _ => {}
                }
            }
        }
        let mut twice = Vec::new();
        walk(&program.statements, &mut HashSet::new(), &mut twice);
        twice
    }

    /// What is wrong with the program's `type`s, if anything — the first of:
    /// a name defined twice (or a predefined one defined again), and a type
    /// that reaches itself without going inside a value (`type A = A ∪
    /// String`), which no value could ever be checked against: asking would
    /// never end. Both backends refuse such a program (`verify.rs`).
    pub fn problem(program: &Program) -> Option<String> {
        if let Some(name) = Types::duplicates(program).first() {
            return Some(format!(
                "type '{name}' is defined more than once — a type's name is the whole program's"
            ));
        }
        let types = Types::of(program);
        let mut names: Vec<&String> = types.defs.keys().collect();
        names.sort();
        names.into_iter().find_map(|name| {
            types
                .reaches_unguarded(&types.defs[name], name, &mut HashSet::new())
                .then(|| {
                    format!(
                        "type '{name}' is itself without anything around it — a type may \
                         contain itself only inside a list or an object, as an element's \
                         children are elements"
                    )
                })
        })
    }

    /// Whether `label` reaches `target` by names and unions alone.
    fn reaches_unguarded(&self, label: &Label, target: &str, seen: &mut HashSet<String>) -> bool {
        match label {
            Label::Named(name) if name == target => true,
            Label::Named(name) => match self.defs.get(name) {
                Some(def) if seen.insert(name.clone()) => self.reaches_unguarded(def, target, seen),
                _ => false,
            },
            Label::Union(members) => members
                .iter()
                .any(|m| self.reaches_unguarded(m, target, seen)),
            _ => false,
        }
    }

    /// What a `type` name stands for, or `None` when no `type` declares it.
    pub fn get(&self, name: &str) -> Option<&Label> {
        self.defs.get(name)
    }

    /// `label` with a declared name replaced by what it stands for, once —
    /// a type's own parts are resolved as they are reached.
    pub fn resolve<'a>(&'a self, label: &'a Label) -> &'a Label {
        let mut at = label;
        let mut seen = HashSet::new();
        while let Label::Named(name) = at {
            match self.defs.get(name) {
                Some(next) if seen.insert(name.as_str()) => at = next,
                _ => break,
            }
        }
        at
    }

    /// The one of the six types every value `label` admits is, or `None`
    /// when it admits more than one (or `Any`). A particle class is Object.
    pub fn kind_of(&self, label: &Label) -> Option<ValueKind> {
        self.kind_within(label, &mut HashSet::new())
    }

    fn kind_within(&self, label: &Label, seen: &mut HashSet<String>) -> Option<ValueKind> {
        match label {
            Label::Named(name) if name == "Any" => None,
            Label::Named(name) => {
                if let Some(kind) = ValueKind::parse(name) {
                    return Some(kind);
                }
                match self.defs.get(name) {
                    Some(def) if seen.insert(name.clone()) => self.kind_within(def, seen),
                    // A type reached again inside itself adds nothing new.
                    Some(_) => None,
                    None => Some(ValueKind::Object),
                }
            }
            Label::Fields(_) | Label::Open(_) | Label::ObjectOf(_) => Some(ValueKind::Object),
            Label::ArrayOf(_) => Some(ValueKind::Array),
            Label::Value(value) => Some(value.kind()),
            Label::Union(members) => {
                let mut kinds = members
                    .iter()
                    .map(|m| self.kind_within(m, &mut seen.clone()));
                let first = kinds.next()??;
                kinds.all(|k| k == Some(first)).then_some(first)
            }
        }
    }

    /// Whether `value` is in the set `label` names — `∈` at run time. null
    /// is in `Null` and `Any` only, as `x ∈ String` has always answered;
    /// a part that is null (a field left out) fits whatever its label is.
    pub fn contains(&self, label: &Label, value: &crate::value::Value) -> bool {
        self.holds(label, value, false)
    }

    fn holds(&self, label: &Label, value: &crate::value::Value, part: bool) -> bool {
        use crate::value::Value;
        if part && matches!(value, Value::Null) {
            return true;
        }
        match label {
            Label::Named(name) => match name.as_str() {
                "Any" => true,
                _ if ValueKind::parse(name).is_some() => {
                    ValueKind::parse(name) == Some(value_kind(value))
                }
                _ => match self.defs.get(name) {
                    Some(def) => self.holds(def, value, part),
                    None => is_particle_of(value, name),
                },
            },
            Label::Value(expected) => match (expected, value) {
                (LabelValue::Str(a), Value::Str(b)) => a.as_str() == &**b,
                (LabelValue::Number(a), Value::Number(b)) => a == b,
                (LabelValue::Bool(a), Value::Bool(b)) => a == b,
                _ => false,
            },
            Label::Union(members) => members.iter().any(|m| self.holds(m, value, part)),
            Label::ArrayOf(element) => match value {
                Value::Array(items) => items.iter().all(|item| self.holds(element, item, true)),
                _ => false,
            },
            Label::ObjectOf(element) => match value {
                Value::Object(fields) => fields.iter().all(|(_, v)| self.holds(element, v, true)),
                _ => false,
            },
            Label::Fields(expected) | Label::Open(expected) => {
                let Value::Object(fields) = value else {
                    return false;
                };
                let closed = matches!(label, Label::Fields(_));
                // A field the value leaves out is null, which fits — except
                // a particle's class: a label that names `_class` is about
                // particles, and an object without one is not a particle.
                // A field the label does not name fits only an open label,
                // or is the class, which is what a particle is, not a field.
                let classed = !expected.iter().any(|(name, _)| name == "_class")
                    || fields
                        .iter()
                        .any(|(key, v)| key == "_class" && !matches!(v, Value::Null));
                classed
                    && fields.iter().all(|(key, v)| {
                        match expected.iter().find(|(name, _)| name == key) {
                            Some((_, field_label)) => self.holds(field_label, v, true),
                            None => !closed || key == "_class",
                        }
                    })
            }
        }
    }
}

fn value_kind(value: &crate::value::Value) -> ValueKind {
    use crate::value::Value;
    match value {
        Value::Number(_) => ValueKind::Number,
        Value::Str(_) => ValueKind::String,
        Value::Bool(_) => ValueKind::Boolean,
        Value::Null => ValueKind::Null,
        Value::Array(_) => ValueKind::Array,
        Value::Object(_) => ValueKind::Object,
    }
}

fn is_particle_of(value: &crate::value::Value, class: &str) -> bool {
    use crate::value::Value;
    let Value::Object(fields) = value else {
        return false;
    };
    fields
        .iter()
        .any(|(key, v)| key == "_class" && matches!(v, Value::Str(name) if &**name == class))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer, parser};

    fn program(source: &str) -> Program {
        let lexed = lexer::tokenize(source).expect("tokenize");
        parser::parse(&lexed).expect("parse")
    }

    #[test]
    fn a_union_of_one_type_is_that_type() {
        let p = program(
            "type Color = \"red\" ∪ \"green\"\ntype Fill = String ∪ { stops ∈ Array of Number }\n",
        );
        let types = Types::of(&p);
        assert_eq!(
            types.kind_of(&Label::Named("Color".into())),
            Some(ValueKind::String)
        );
        assert_eq!(types.kind_of(&Label::Named("Fill".into())), None);
        assert_eq!(
            types.kind_of(&Label::Named("Point".into())),
            Some(ValueKind::Object)
        );
    }

    #[test]
    fn a_type_may_contain_itself() {
        let p =
            program("type Element = { tag ∈ String, children ∈ Array of (String ∪ Element) }\n");
        let types = Types::of(&p);
        assert_eq!(
            types.kind_of(&Label::Named("Element".into())),
            Some(ValueKind::Object)
        );
    }
}

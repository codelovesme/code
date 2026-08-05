use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::ast::TypeExpr;

/// A constraint domain: the set of possible values a constrained variable can hold.
/// Domains are intersected as constraints are applied; variables resolve to concrete
/// values lazily when their value is needed.
#[derive(Debug, Clone)]
pub enum Domain {
    /// Unconstrained — any value is allowed.
    Any,
    /// Exactly one concrete value (equivalent to old assignment).
    Exact(Rc<Value>),
    /// Integer range: min..=max (either bound can be open).
    IntegerRange {
        min: Option<i64>,
        max: Option<i64>,
    },
    /// Real number range with inclusive/exclusive bounds.
    RealRange {
        min: Option<f64>,
        max: Option<f64>,
        min_inclusive: bool,
        max_inclusive: bool,
    },
    /// A finite set of allowed values.
    ValueSet(Vec<Rc<Value>>),
    /// A type domain: variable must be of this type.
    TypeDomain(TypeExpr),
    /// Structural object-schema membership (T26 Phase 2): variable must be
    /// an object satisfying every named field's domain. Open/structural —
    /// extra fields beyond the ones listed are allowed (this is what lets
    /// `∩` work as inheritance: `A ∩ B` is genuinely satisfiable by an
    /// object with both A's and B's fields, not just objects with *only*
    /// those fields).
    Schema(HashMap<String, Domain>),
    /// Discriminated union (T26 Phase 3): variable must satisfy *at least
    /// one* of these domains — e.g. `s ∈ Status` where `Status = {"Success"}
    /// ∪ {tag = "Error", code ∈ Number}` means s is either the string
    /// "Success" or an object matching the error schema. Members are
    /// pre-flattened (a `Union` never directly contains another `Union`).
    Union(Vec<Domain>),
    /// Intersection of multiple domains.
    Intersection(Vec<Domain>),
    /// Empty domain — unsatisfiable (contradictory constraints).
    Empty,
}

impl Domain {
    /// Check if this domain contains exactly one value.
    pub fn is_singleton(&self) -> Option<Rc<Value>> {
        match self {
            Domain::Exact(v) => Some(Rc::clone(v)),
            Domain::ValueSet(vs) if vs.len() == 1 => Some(Rc::clone(&vs[0])),
            Domain::IntegerRange { min: Some(a), max: Some(b) } if *a == *b => {
                Some(Value::number(*a as f64))
            }
            Domain::Intersection(parts) => {
                // If any part is Exact, return that value
                for part in parts {
                    if let Some(v) = part.is_singleton() {
                        return Some(v);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Check if this domain is empty (unsatisfiable).
    pub fn is_empty_domain(&self) -> bool {
        matches!(self, Domain::Empty)
    }

    /// Intersect this domain with another constraint, producing a narrower domain.
    pub fn intersect(self, other: Domain) -> Domain {
        match (&self, &other) {
            (Domain::Empty, _) | (_, Domain::Empty) => Domain::Empty,
            (Domain::Any, _) => other,
            (_, Domain::Any) => self,
            (Domain::Exact(v1), Domain::Exact(v2)) => {
                if values_equal(v1, v2) {
                    self
                } else {
                    Domain::Empty
                }
            }
            // Exact + RealRange: check if the exact value satisfies the range
            (Domain::Exact(v), Domain::RealRange { min, max, min_inclusive, max_inclusive }) => {
                if let Value::Number(n) = v.as_ref() {
                    if real_range_contains(*n, min, max, *min_inclusive, *max_inclusive) {
                        self
                    } else {
                        Domain::Empty
                    }
                } else {
                    Domain::Empty
                }
            }
            (Domain::RealRange { min, max, min_inclusive, max_inclusive }, Domain::Exact(v)) => {
                if let Value::Number(n) = v.as_ref() {
                    if real_range_contains(*n, min, max, *min_inclusive, *max_inclusive) {
                        other
                    } else {
                        Domain::Empty
                    }
                } else {
                    Domain::Empty
                }
            }
            // RealRange + RealRange: compute the tighter bounds
            (
                Domain::RealRange { min: min1, max: max1, min_inclusive: mi1, max_inclusive: mxi1 },
                Domain::RealRange { min: min2, max: max2, min_inclusive: mi2, max_inclusive: mxi2 },
            ) => {
                let (new_min, new_mi) = merge_lower_bound(min1, *mi1, min2, *mi2);
                let (new_max, new_mxi) = merge_upper_bound(max1, *mxi1, max2, *mxi2);
                // Check for empty range
                if let (Some(lo), Some(hi)) = (new_min, new_max) {
                    if lo > hi || (lo == hi && !(new_mi && new_mxi)) {
                        return Domain::Empty;
                    }
                }
                Domain::RealRange {
                    min: new_min,
                    max: new_max,
                    min_inclusive: new_mi,
                    max_inclusive: new_mxi,
                }
            }
            // Exact + TypeDomain(Named(builtin)): actually check the value
            // matches — found while building T26 Phase 2's object-schema
            // satisfaction check, which depends on this: `m ∈ Number` must
            // genuinely reject `m = {1,2}` (a Set), not pass it through
            // unconditionally. Custom `type X {...}` names still pass
            // through loose (`None` case) — checking those needs an
            // Interpreter's type registry, which this free function doesn't
            // have; that stays the pre-existing behavior.
            (Domain::Exact(v), Domain::TypeDomain(TypeExpr::Named(name))) => {
                match value_matches_builtin_type_name(v, name) {
                    Some(false) => Domain::Empty,
                    _ => self,
                }
            }
            (Domain::TypeDomain(TypeExpr::Named(name)), Domain::Exact(v)) => {
                match value_matches_builtin_type_name(v, name) {
                    Some(false) => Domain::Empty,
                    _ => other,
                }
            }
            // Exact + TypeDomain (any other TypeExpr shape — Union,
            // Intersection, Literal): unchanged loose pass-through.
            (Domain::Exact(_), Domain::TypeDomain(_)) => self,
            (Domain::TypeDomain(_), Domain::Exact(_)) => other,
            // Exact + IntegerRange: check the exact value is a whole number in range
            (Domain::Exact(v), Domain::IntegerRange { min, max }) => {
                if let Value::Number(n) = v.as_ref() {
                    if integer_range_contains(*n, min, max) {
                        self
                    } else {
                        Domain::Empty
                    }
                } else {
                    Domain::Empty
                }
            }
            (Domain::IntegerRange { min, max }, Domain::Exact(v)) => {
                if let Value::Number(n) = v.as_ref() {
                    if integer_range_contains(*n, min, max) {
                        other
                    } else {
                        Domain::Empty
                    }
                } else {
                    Domain::Empty
                }
            }
            // IntegerRange + IntegerRange: tighter bounds
            (
                Domain::IntegerRange { min: min1, max: max1 },
                Domain::IntegerRange { min: min2, max: max2 },
            ) => {
                let new_min = merge_int_lower(min1, min2);
                let new_max = merge_int_upper(max1, max2);
                if let (Some(lo), Some(hi)) = (new_min, new_max) {
                    if lo > hi {
                        return Domain::Empty;
                    }
                }
                Domain::IntegerRange { min: new_min, max: new_max }
            }
            // IntegerRange + RealRange: e.g. `a in Z` combined with `a < 2, a > 0` —
            // convert the real bounds to the tightest integer bounds they imply,
            // then merge. This is what lets `a in Z; a < 2; a > 0` resolve to {1}
            // instead of getting stuck as an unresolved intersection.
            (
                Domain::IntegerRange { min, max },
                Domain::RealRange { min: rmin, max: rmax, min_inclusive, max_inclusive },
            ) => {
                let (conv_min, conv_max) =
                    real_bounds_to_integer_bounds(rmin, *min_inclusive, rmax, *max_inclusive);
                let new_min = merge_int_lower(min, &conv_min);
                let new_max = merge_int_upper(max, &conv_max);
                if let (Some(lo), Some(hi)) = (new_min, new_max) {
                    if lo > hi {
                        return Domain::Empty;
                    }
                }
                Domain::IntegerRange { min: new_min, max: new_max }
            }
            (
                Domain::RealRange { min: rmin, max: rmax, min_inclusive, max_inclusive },
                Domain::IntegerRange { min, max },
            ) => {
                let (conv_min, conv_max) =
                    real_bounds_to_integer_bounds(rmin, *min_inclusive, rmax, *max_inclusive);
                let new_min = merge_int_lower(min, &conv_min);
                let new_max = merge_int_upper(max, &conv_max);
                if let (Some(lo), Some(hi)) = (new_min, new_max) {
                    if lo > hi {
                        return Domain::Empty;
                    }
                }
                Domain::IntegerRange { min: new_min, max: new_max }
            }
            // Exact + ValueSet: check the exact value is one of the set's members.
            (Domain::Exact(v), Domain::ValueSet(items)) => {
                if items.iter().any(|it| values_equal(it, v)) {
                    self
                } else {
                    Domain::Empty
                }
            }
            (Domain::ValueSet(items), Domain::Exact(v)) => {
                if items.iter().any(|it| values_equal(it, v)) {
                    other
                } else {
                    Domain::Empty
                }
            }
            // ValueSet + ValueSet: keep only members present in both.
            (Domain::ValueSet(a), Domain::ValueSet(b)) => {
                let kept: Vec<Rc<Value>> = a
                    .iter()
                    .filter(|av| b.iter().any(|bv| values_equal(av, bv)))
                    .cloned()
                    .collect();
                if kept.is_empty() {
                    Domain::Empty
                } else {
                    Domain::ValueSet(kept)
                }
            }
            // ValueSet + RealRange/IntegerRange: keep only numeric members
            // that satisfy the range — e.g. `x ∈ {1,2,3}; x > 1` collapses to
            // {2,3} instead of getting stuck as an unresolved intersection.
            (
                Domain::ValueSet(items),
                Domain::RealRange { min, max, min_inclusive, max_inclusive },
            ) => value_set_intersect_real_range(items, min, max, *min_inclusive, *max_inclusive),
            (
                Domain::RealRange { min, max, min_inclusive, max_inclusive },
                Domain::ValueSet(items),
            ) => value_set_intersect_real_range(items, min, max, *min_inclusive, *max_inclusive),
            (Domain::ValueSet(items), Domain::IntegerRange { min, max }) => {
                value_set_intersect_integer_range(items, min, max)
            }
            (Domain::IntegerRange { min, max }, Domain::ValueSet(items)) => {
                value_set_intersect_integer_range(items, min, max)
            }
            // Schema + Exact: does this concrete object satisfy every field
            // constraint the schema names? (Open/structural — extra fields
            // on the object beyond what the schema constrains are fine, per
            // T26 Decision 3.) This is what lets `mm ∈ K; mm = {...}`
            // resolve when the object matches, and contradict when it
            // doesn't — same mechanism as every other domain kind.
            (Domain::Schema(schema), Domain::Exact(v)) => match v.as_ref() {
                Value::Object(obj_fields) if object_satisfies_schema(obj_fields, schema) => other,
                _ => Domain::Empty,
            },
            (Domain::Exact(v), Domain::Schema(schema)) => match v.as_ref() {
                Value::Object(obj_fields) if object_satisfies_schema(obj_fields, schema) => self,
                _ => Domain::Empty,
            },
            // Schema + Schema: merge field constraints (union of field
            // names; intersect the domain for any field both name). This is
            // `∩`-as-inheritance's underlying mechanism (T26 Decision 3) —
            // `mm ∈ K1; mm ∈ K2` must satisfy both.
            (Domain::Schema(a), Domain::Schema(b)) => match merge_schemas(a.clone(), b.clone()) {
                Some(merged) => Domain::Schema(merged),
                None => Domain::Empty,
            },
            // Union + Exact: resolves if the pinned value satisfies *any*
            // alternative (discriminated union — T26 Phase 3), e.g.
            // `s ∈ Status; s = "Success"` or `s = {tag="Error", code=1}`.
            // Empty only if it matches none of them.
            (Domain::Union(parts), Domain::Exact(v)) => {
                let matches_any = parts.iter().any(|p| {
                    !p.clone().intersect(Domain::Exact(Rc::clone(v))).is_empty_domain()
                });
                if matches_any { other } else { Domain::Empty }
            }
            (Domain::Exact(v), Domain::Union(parts)) => {
                let matches_any = parts.iter().any(|p| {
                    !p.clone().intersect(Domain::Exact(Rc::clone(v))).is_empty_domain()
                });
                if matches_any { self } else { Domain::Empty }
            }
            _ => {
                // General case: wrap in Intersection
                let mut parts = Vec::new();
                match self {
                    Domain::Intersection(v) => parts.extend(v),
                    other => parts.push(other),
                }
                match other {
                    Domain::Intersection(v) => parts.extend(v),
                    other => parts.push(other),
                }
                Domain::Intersection(parts)
            }
        }
    }

    /// Describe this domain in human terms for a diagnostic — used when a
    /// variable exists but hasn't narrowed to a single value yet. Lists the
    /// possible values when the domain is small and finite, otherwise
    /// describes the constraint itself.
    pub fn describe(&self) -> String {
        const MAX_LISTED: i64 = 20;
        match self {
            Domain::Exact(v) => format!("{}", v),
            Domain::Any => "unconstrained".to_string(),
            Domain::Empty => "contradictory — no possible values".to_string(),
            Domain::ValueSet(vs) => {
                let items: Vec<String> = vs.iter().map(|v| format!("{}", v)).collect();
                format!("possible values: {{{}}}", items.join(", "))
            }
            Domain::IntegerRange { min, max } => match (min, max) {
                (Some(lo), Some(hi)) if hi - lo <= MAX_LISTED => {
                    let items: Vec<String> = (*lo..=*hi).map(|n| n.to_string()).collect();
                    format!("possible values: {{{}}}", items.join(", "))
                }
                (Some(lo), Some(hi)) => format!("{} ≤ _ ≤ {} (integers)", lo, hi),
                (Some(lo), None) => format!("_ ≥ {} (integers)", lo),
                (None, Some(hi)) => format!("_ ≤ {} (integers)", hi),
                (None, None) => "any integer".to_string(),
            },
            Domain::RealRange { min, max, min_inclusive, max_inclusive } => {
                let lo_op = if *min_inclusive { "≤" } else { "<" };
                let hi_op = if *max_inclusive { "≤" } else { "<" };
                match (min, max) {
                    (Some(lo), Some(hi)) => format!("{} {} _ {} {}", lo, lo_op, hi_op, hi),
                    (Some(lo), None) => format!("_ {} {}", if *min_inclusive { "≥" } else { ">" }, lo),
                    (None, Some(hi)) => format!("_ {} {}", hi_op, hi),
                    (None, None) => "any number".to_string(),
                }
            }
            Domain::TypeDomain(t) => format!("must be of type {}", t),
            Domain::Schema(fields) => {
                let mut names: Vec<&String> = fields.keys().collect();
                names.sort();
                let items: Vec<String> = names
                    .into_iter()
                    .map(|k| format!("{} ∈ ({})", k, fields[k].describe()))
                    .collect();
                format!("must be an object with {{ {} }}", items.join(", "))
            }
            Domain::Union(parts) => {
                let items: Vec<String> = parts.iter().map(|p| p.describe()).collect();
                format!("must be one of: ({})", items.join(") or ("))
            }
            Domain::Intersection(parts) => {
                let items: Vec<String> = parts.iter().map(|p| p.describe()).collect();
                items.join(" and ")
            }
        }
    }

    /// Enumerate this domain's members, for `loop <var> { }` (T26) —
    /// enumerating a variable's own domain in place. Only finite domains can
    /// be enumerated: a `ValueSet`, a bounded `IntegerRange`, an already-
    /// resolved `Exact` (a trivial one-candidate loop), a `Union` whose
    /// every member is itself finite, a `Schema` whose every field is
    /// itself finite (enumerated as the Cartesian product of its fields —
    /// the canonical minimal objects with exactly those fields, even though
    /// the schema's structural membership is open-ended), or an
    /// `Intersection` where at least one part is finite (the rest are used
    /// as filters). An unbounded `IntegerRange` and *any* `RealRange` are
    /// rejected — even a bounded real range has infinitely many values
    /// (uncountably many between any two reals) — as is a bare `TypeDomain`
    /// naming a builtin (`Number`, `String`, …), which is unbounded by
    /// definition.
    pub fn finite_candidates(&self) -> Result<Vec<Rc<Value>>, String> {
        match self {
            Domain::Exact(v) => Ok(vec![Rc::clone(v)]),
            Domain::ValueSet(items) => Ok(items.clone()),
            Domain::IntegerRange { min: Some(lo), max: Some(hi) } => {
                Ok((*lo..=*hi).map(|n| Value::number(n as f64)).collect())
            }
            Domain::Union(parts) => {
                let mut out = Vec::new();
                for part in parts {
                    out.extend(part.finite_candidates()?);
                }
                Ok(out)
            }
            Domain::Schema(fields) => {
                // Cartesian product over each field's own finite candidates,
                // sorted by name first for deterministic output order (the
                // schema is a HashMap, so iteration order alone isn't
                // stable). One Object per combination.
                let mut names: Vec<&String> = fields.keys().collect();
                names.sort();
                let mut combos: Vec<HashMap<String, Rc<Value>>> = vec![HashMap::new()];
                for name in names {
                    let field_candidates = fields[name]
                        .finite_candidates()
                        .map_err(|e| format!("field '{}' of this schema: {}", name, e))?;
                    let mut next = Vec::with_capacity(combos.len() * field_candidates.len());
                    for partial in &combos {
                        for candidate in &field_candidates {
                            let mut m = partial.clone();
                            m.insert(name.clone(), Rc::clone(candidate));
                            next.push(m);
                        }
                    }
                    combos = next;
                }
                Ok(combos.into_iter().map(Value::object).collect())
            }
            Domain::Intersection(parts) => {
                // Enumerate from whichever part is finite on its own, then
                // keep only the candidates that also satisfy every other
                // part — same containment check the Union+Exact narrowing
                // arm above uses (intersect against a singleton, see if
                // anything survives).
                let base = parts.iter().enumerate().find_map(|(i, p)| {
                    p.finite_candidates().ok().map(|c| (i, c))
                });
                let Some((base_idx, candidates)) = base else {
                    return Err(format!(
                        "cannot enumerate {} — none of its combined constraints is finite \
                         on its own",
                        self.describe()
                    ));
                };
                Ok(candidates
                    .into_iter()
                    .filter(|v| {
                        parts.iter().enumerate().all(|(i, p)| {
                            i == base_idx
                                || !p
                                    .clone()
                                    .intersect(Domain::Exact(Rc::clone(v)))
                                    .is_empty_domain()
                        })
                    })
                    .collect())
            }
            _ => Err(format!(
                "cannot enumerate an infinite or unbounded domain ({}) — only a finite \
                 set of possible values (or a bounded integer range, or a finite union/\
                 schema/intersection built from those) can be enumerated",
                self.describe()
            )),
        }
    }
}

/// Check if a number falls within a real range.
fn real_range_contains(
    n: f64,
    min: &Option<f64>,
    max: &Option<f64>,
    min_inclusive: bool,
    max_inclusive: bool,
) -> bool {
    if let Some(lo) = min {
        if min_inclusive { if n < *lo { return false; } }
        else { if n <= *lo { return false; } }
    }
    if let Some(hi) = max {
        if max_inclusive { if n > *hi { return false; } }
        else { if n >= *hi { return false; } }
    }
    true
}

/// Merge two lower bounds, picking the tighter one.
fn merge_lower_bound(a: &Option<f64>, ai: bool, b: &Option<f64>, bi: bool) -> (Option<f64>, bool) {
    match (a, b) {
        (None, None) => (None, false),
        (Some(v), None) => (Some(*v), ai),
        (None, Some(v)) => (Some(*v), bi),
        (Some(va), Some(vb)) => {
            if va > vb { (Some(*va), ai) }
            else if vb > va { (Some(*vb), bi) }
            else { (Some(*va), ai && bi) } // same bound: inclusive only if both are
        }
    }
}

/// Merge two upper bounds, picking the tighter one.
fn merge_upper_bound(a: &Option<f64>, ai: bool, b: &Option<f64>, bi: bool) -> (Option<f64>, bool) {
    match (a, b) {
        (None, None) => (None, false),
        (Some(v), None) => (Some(*v), ai),
        (None, Some(v)) => (Some(*v), bi),
        (Some(va), Some(vb)) => {
            if va < vb { (Some(*va), ai) }
            else if vb < va { (Some(*vb), bi) }
            else { (Some(*va), ai && bi) }
        }
    }
}

/// Check whether a number is a whole number falling within an integer range.
fn integer_range_contains(n: f64, min: &Option<i64>, max: &Option<i64>) -> bool {
    if n.fract() != 0.0 {
        return false;
    }
    let n_i = n as i64;
    if let Some(lo) = min {
        if n_i < *lo { return false; }
    }
    if let Some(hi) = max {
        if n_i > *hi { return false; }
    }
    true
}

/// Pick the tighter (larger) of two optional integer lower bounds.
fn merge_int_lower(a: &Option<i64>, b: &Option<i64>) -> Option<i64> {
    match (a, b) {
        (None, None) => None,
        (Some(v), None) | (None, Some(v)) => Some(*v),
        (Some(va), Some(vb)) => Some((*va).max(*vb)),
    }
}

/// Pick the tighter (smaller) of two optional integer upper bounds.
fn merge_int_upper(a: &Option<i64>, b: &Option<i64>) -> Option<i64> {
    match (a, b) {
        (None, None) => None,
        (Some(v), None) | (None, Some(v)) => Some(*v),
        (Some(va), Some(vb)) => Some((*va).min(*vb)),
    }
}

/// Filter a value-set down to the numeric members satisfying a real range,
/// producing the narrower domain (or `Empty` if nothing survives).
fn value_set_intersect_real_range(
    items: &[Rc<Value>],
    min: &Option<f64>,
    max: &Option<f64>,
    min_inclusive: bool,
    max_inclusive: bool,
) -> Domain {
    let kept: Vec<Rc<Value>> = items
        .iter()
        .filter(|v| match v.as_ref() {
            Value::Number(n) => real_range_contains(*n, min, max, min_inclusive, max_inclusive),
            _ => false,
        })
        .cloned()
        .collect();
    if kept.is_empty() {
        Domain::Empty
    } else {
        Domain::ValueSet(kept)
    }
}

/// Filter a value-set down to the whole-number members within an integer
/// range, producing the narrower domain (or `Empty` if nothing survives).
fn value_set_intersect_integer_range(
    items: &[Rc<Value>],
    min: &Option<i64>,
    max: &Option<i64>,
) -> Domain {
    let kept: Vec<Rc<Value>> = items
        .iter()
        .filter(|v| match v.as_ref() {
            Value::Number(n) => integer_range_contains(*n, min, max),
            _ => false,
        })
        .cloned()
        .collect();
    if kept.is_empty() {
        Domain::Empty
    } else {
        Domain::ValueSet(kept)
    }
}

/// Best-effort check of whether a value matches a *built-in* type name
/// (Number/String/Boolean/Object/Array/Set/Schema/Null/Any). `None` means
/// `name` isn't a recognized built-in — likely a custom `type X {...}` name,
/// which needs an Interpreter's type registry (structural `_class`
/// matching) that this free function doesn't have access to; callers treat
/// `None` as "can't rule it out" and pass the value through.
fn value_matches_builtin_type_name(val: &Value, name: &str) -> Option<bool> {
    match name {
        "Number" => Some(matches!(val, Value::Number(_))),
        "String" => Some(matches!(val, Value::String(_))),
        "Boolean" => Some(matches!(val, Value::Boolean(_))),
        "Object" => Some(matches!(val, Value::Object(_))),
        "Array" => Some(matches!(val, Value::Array(_))),
        "Set" => Some(matches!(val, Value::Set(_))),
        "Schema" => Some(matches!(val, Value::Schema(_))),
        "Union" => Some(matches!(val, Value::Union(_))),
        "Null" => Some(matches!(val, Value::Null)),
        "Any" => Some(true),
        _ => None,
    }
}

/// Does this concrete object satisfy every field constraint a schema names?
/// Open/structural (T26 Decision 3): a missing field fails, but the object
/// may have *extra* fields beyond what the schema constrains.
fn object_satisfies_schema(
    obj_fields: &HashMap<String, Rc<Value>>,
    schema: &HashMap<String, Domain>,
) -> bool {
    schema.iter().all(|(name, domain)| {
        obj_fields
            .get(name)
            .map(|v| !domain.clone().intersect(Domain::Exact(Rc::clone(v))).is_empty_domain())
            .unwrap_or(false)
    })
}

/// Merge two schemas' field constraints: union of field names, intersecting
/// the domain for any field both name. `None` if intersecting any shared
/// field's domain is contradictory (T26 Decision 3 — this is `∩`'s
/// underlying mechanism for object-schema inheritance).
pub(crate) fn merge_schemas(
    mut a: HashMap<String, Domain>,
    b: HashMap<String, Domain>,
) -> Option<HashMap<String, Domain>> {
    for (k, bd) in b {
        match a.remove(&k) {
            Some(ad) => {
                let merged = ad.intersect(bd);
                if merged.is_empty_domain() {
                    return None;
                }
                a.insert(k, merged);
            }
            None => {
                a.insert(k, bd);
            }
        }
    }
    Some(a)
}

/// Express a `Set`/`Schema`/`Union` value as a flat list of `Domain`
/// alternatives, for building `∪` (T26 Phase 3): a `Set`'s elements become
/// one `ValueSet` member, a `Schema` becomes one `Schema` member, and a
/// `Union` is already such a list (flattened in, never nested). `None` for
/// any other value kind — `∪` requires both sides to be one of these three.
pub(crate) fn value_to_union_members(v: &Value) -> Option<Vec<Domain>> {
    match v {
        Value::Set(items) => Some(vec![Domain::ValueSet(items.clone())]),
        Value::Schema(fields) => Some(vec![Domain::Schema(fields.clone())]),
        Value::Union(parts) => Some(parts.clone()),
        _ => None,
    }
}

/// Express *any* value as the domain it denotes for `∈`'s right side
/// (T26 follow-up): a `Set`/`Array`'s elements are the candidates (the
/// same "legacy convenience" `in [1, 2, 3]` always had — checking against
/// the elements, not against the array as one opaque value), a `Schema`
/// checks structural satisfaction, a `Union` checks any alternative — and
/// anything else (a plain `Number`, `Object`, …) denotes the singleton
/// set containing just itself, so `x ∈ y` means `x = y`. Unlike
/// `value_to_union_members`, this never fails: "everything is a set"
/// (T26) applies uniformly to `∈`'s container, not only to the three
/// possibility-space kinds `∪`/`∩` require.
pub(crate) fn value_as_membership_domain(container: &Value) -> Domain {
    match container {
        Value::Array(items) | Value::Set(items) => Domain::ValueSet(items.clone()),
        Value::Schema(fields) => Domain::Schema(fields.clone()),
        Value::Union(parts) => Domain::Union(parts.clone()),
        other => Domain::Exact(Rc::new(other.clone())),
    }
}

/// Outcome of testing an unresolved variable's domain against a type check
/// for `if`-narrowing (T26 Phase 3b): decided either way, or genuinely
/// mixed — in which case `Narrowed` carries the domain to use *inside* the
/// `if`-true branch (block-scoped; the outer variable is untouched, same
/// rule as `loop <var> { }` in Phase 1).
pub(crate) enum DomainSplit {
    AlwaysTrue,
    AlwaysFalse,
    Narrowed(Domain),
}

/// Split one domain "alternative" (a `Union` member, or a whole non-Union
/// domain treated as a single alternative) into the part that satisfies a
/// built-in type name and the part that doesn't. `None` on a side means
/// nothing in this alternative falls there. Conservative for domain kinds
/// with no clear split (custom `type X {...}` names, `Any`, `Intersection`):
/// treated as fully matching, so narrowing just skips them rather than
/// risking an incorrect split.
fn partition_domain_member(m: &Domain, type_name: &str) -> (Option<Domain>, Option<Domain>) {
    match m {
        Domain::Exact(v) => match value_matches_builtin_type_name(v, type_name) {
            Some(true) => (Some(m.clone()), None),
            Some(false) => (None, Some(m.clone())),
            None => (Some(m.clone()), None),
        },
        Domain::ValueSet(items) => {
            let (matching, non_matching): (Vec<Rc<Value>>, Vec<Rc<Value>>) = items
                .iter()
                .cloned()
                .partition(|v| value_matches_builtin_type_name(v, type_name) != Some(false));
            (
                (!matching.is_empty()).then(|| Domain::ValueSet(matching)),
                (!non_matching.is_empty()).then(|| Domain::ValueSet(non_matching)),
            )
        }
        Domain::Schema(_) => match type_name {
            "Object" | "Any" => (Some(m.clone()), None),
            _ => (None, Some(m.clone())),
        },
        Domain::IntegerRange { .. } | Domain::RealRange { .. } => match type_name {
            "Number" | "Any" => (Some(m.clone()), None),
            _ => (None, Some(m.clone())),
        },
        Domain::TypeDomain(TypeExpr::Named(n)) if n == type_name => (Some(m.clone()), None),
        // Everything else (Any, Intersection, non-matching/custom TypeDomain
        // names, a stray nested Union) — can't safely disprove, so treat as
        // fully matching (no narrowing lost on the true side, and it's
        // simply not excluded on the false side either).
        _ => (Some(m.clone()), Some(m.clone())),
    }
}

/// Decide `variable ∈/∉ type_name` from the variable's domain, and (for the
/// mixed case) the domain to use inside the matching `if`-branch. Handles a
/// `Domain::Union` by evaluating per-alternative and recombining; any other
/// domain is treated as a single alternative.
pub(crate) fn split_domain_by_type_name(
    domain: &Domain,
    type_name: &str,
    negated: bool,
) -> DomainSplit {
    let members: Vec<Domain> = match domain {
        Domain::Union(parts) => parts.clone(),
        other => vec![other.clone()],
    };
    let mut kept: Vec<Domain> = Vec::new();
    let mut all_kept = true;
    let mut none_kept = true;
    for m in &members {
        let (matching, non_matching) = partition_domain_member(m, type_name);
        let (side, other_empty) = if negated {
            (non_matching, matching.is_none())
        } else {
            (matching, non_matching.is_none())
        };
        match side {
            Some(d) => {
                none_kept = false;
                if !other_empty {
                    all_kept = false;
                }
                kept.push(d);
            }
            None => all_kept = false,
        }
    }
    if all_kept {
        DomainSplit::AlwaysTrue
    } else if none_kept {
        DomainSplit::AlwaysFalse
    } else if kept.len() == 1 {
        DomainSplit::Narrowed(kept.into_iter().next().unwrap())
    } else {
        DomainSplit::Narrowed(Domain::Union(kept))
    }
}

/// Convert real-valued bounds (from `<`/`>`/`≤`/`≥` constraints) into the
/// tightest integer bounds they imply — e.g. `a < 2` (exclusive real upper
/// bound 2) implies the integer upper bound is 1.
fn real_bounds_to_integer_bounds(
    min: &Option<f64>,
    min_inclusive: bool,
    max: &Option<f64>,
    max_inclusive: bool,
) -> (Option<i64>, Option<i64>) {
    let lo = min.map(|v| {
        if min_inclusive { v.ceil() as i64 } else { v.floor() as i64 + 1 }
    });
    let hi = max.map(|v| {
        if max_inclusive { v.floor() as i64 } else { v.ceil() as i64 - 1 }
    });
    (lo, hi)
}

/// Runtime value representation for Code.
/// All values live on the heap via Rc<Value>.
/// Values are immutable after creation — reassignment creates a new heap value.
#[derive(Debug, Clone)]
pub enum Value {
    Number(f64),
    String(String),
    Boolean(bool),
    Object(HashMap<String, Rc<Value>>),
    Array(Vec<Rc<Value>>),
    /// A set: unordered, deduplicated (via `values_equal`). Unlike `Array`
    /// (an ordered "values list" that allows duplicates), a set element
    /// never repeats — see T26. Element order here is insertion order after
    /// dedup, kept only for deterministic `Display`/iteration, not meaning.
    Set(Vec<Rc<Value>>),
    /// An object-schema, resolved as a value in its own right (T26 Phase
    /// 2): `K = { k ∈ KK, m ∈ Number }` binds K to *this* schema — one
    /// well-defined value — even though the set of objects K describes is
    /// open-ended (potentially infinite, since e.g. `Number` has infinitely
    /// many members). Distinct from `Object`: a `Schema`'s fields are
    /// per-field `Domain`s (constraints), not resolved values. `abc ∈ K`
    /// narrows `abc`'s domain to "objects satisfying K" — see
    /// `Domain::Schema`.
    Schema(HashMap<String, Domain>),
    /// Discriminated union (T26 Phase 3): `Status = {"Success"} ∪ {tag =
    /// "Error", code ∈ Number}` binds Status to *this* union — one
    /// well-defined value, resolved the same way a `Set`/`Schema` is,
    /// even though what it describes is "one of several alternative
    /// shapes." Members are pre-flattened Domains (see `Domain::Union`).
    Union(Vec<Domain>),
    Null,
}

impl Value {
    /// Create a heap-allocated Number value.
    pub fn number(n: f64) -> Rc<Value> {
        Rc::new(Value::Number(n))
    }

    /// Create a heap-allocated String value.
    pub fn string(s: impl Into<String>) -> Rc<Value> {
        Rc::new(Value::String(s.into()))
    }

    /// Create a heap-allocated Boolean value.
    pub fn boolean(b: bool) -> Rc<Value> {
        Rc::new(Value::Boolean(b))
    }

    /// Create a heap-allocated Object value.
    pub fn object(fields: HashMap<String, Rc<Value>>) -> Rc<Value> {
        Rc::new(Value::Object(fields))
    }

    /// Create a heap-allocated Array value.
    pub fn array(elements: Vec<Rc<Value>>) -> Rc<Value> {
        Rc::new(Value::Array(elements))
    }

    /// Create a heap-allocated Set value, deduplicating elements by deep
    /// equality (first occurrence wins, order otherwise preserved).
    pub fn set(elements: Vec<Rc<Value>>) -> Rc<Value> {
        let mut deduped: Vec<Rc<Value>> = Vec::with_capacity(elements.len());
        for el in elements {
            if !deduped.iter().any(|existing| values_equal(existing, &el)) {
                deduped.push(el);
            }
        }
        Rc::new(Value::Set(deduped))
    }

    /// Create a heap-allocated Schema value (T26 Phase 2).
    pub fn schema(fields: HashMap<String, Domain>) -> Rc<Value> {
        Rc::new(Value::Schema(fields))
    }

    /// Create a heap-allocated Null value.
    pub fn null() -> Rc<Value> {
        Rc::new(Value::Null)
    }

    /// Return the Code type name for this value (used for type checking).
    pub fn type_name(&self) -> &str {
        match self {
            Value::Number(_) => "Number",
            Value::String(_) => "String",
            Value::Boolean(_) => "Boolean",
            Value::Object(_) => "Object",
            Value::Array(_) => "Array",
            Value::Set(_) => "Set",
            Value::Schema(_) => "Schema",
            Value::Union(_) => "Union",
            Value::Null => "Null",
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(n) => write!(f, "{}", n),
            Value::String(s) => write!(f, "{}", s),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Object(fields) => {
                write!(f, "{{ ")?;
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{} = {}", k, v)?;
                }
                write!(f, " }}")
            }
            Value::Array(elements) => {
                write!(f, "[")?;
                for (i, v) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Value::Set(elements) => {
                write!(f, "{{")?;
                for (i, v) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "}}")
            }
            Value::Schema(fields) => {
                let mut names: Vec<&String> = fields.keys().collect();
                names.sort();
                write!(f, "{{")?;
                for (i, k) in names.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{} ∈ ({})", k, fields[*k].describe())?;
                }
                write!(f, "}}")
            }
            Value::Union(parts) => {
                let items: Vec<String> = parts.iter().map(|p| p.describe()).collect();
                write!(f, "({})", items.join(") ∪ ("))
            }
            Value::Null => write!(f, "Null"),
        }
    }
}

/// Deep equality comparison for Values.
pub fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Boolean(a), Value::Boolean(b)) => a == b,
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return false;
            }
            a.iter().zip(b.iter()).all(|(av, bv)| values_equal(av, bv))
        }
        (Value::Object(a), Value::Object(b)) => {
            if a.len() != b.len() {
                return false;
            }
            a.iter().all(|(k, v)| {
                b.get(k)
                    .map(|bv| values_equal(v, bv))
                    .unwrap_or(false)
            })
        }
        (Value::Set(a), Value::Set(b)) => {
            // Both sides are already deduplicated (Value::set()'s invariant),
            // so same length + every element of a found in b is sufficient —
            // no need for order or a bijection search.
            if a.len() != b.len() {
                return false;
            }
            a.iter()
                .all(|av| b.iter().any(|bv| values_equal(av, bv)))
        }
        _ => false,
    }
}

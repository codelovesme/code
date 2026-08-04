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
            // Exact + TypeDomain: keep the exact if it matches (loose check)
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
            Domain::Intersection(parts) => {
                let items: Vec<String> = parts.iter().map(|p| p.describe()).collect();
                items.join(" and ")
            }
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
        _ => false,
    }
}

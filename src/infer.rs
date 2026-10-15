//! Kinds fixed by their first assignment (ticket 122, stage A.2).
//!
//! A variable keeps the kind it first held, and so does every field of an
//! object and every element of a list: `person.age = 24` makes `age` a Number
//! for good. null is not a kind here — a value not set yet is null, whatever
//! it will hold — and an empty list's elements are what first joins it. A
//! list written with several types in it (`[name, icon]`) holds all of them
//! from the start. This works out every one of those kinds over the whole
//! file and reports a later assignment of another kind as an error: in
//! `code check`, and `verify.rs` refuses to run or build the program (A.4).
//! A value that is several types on purpose says so where it starts, with a
//! label: `controls ∈ Array of Element = []`.
//!
//! **Values flow, so kinds follow the flow.** `b = a`, two objects put in one
//! list, a particle emitted to a handler: whatever one value can reach has
//! one kind, so the analysis keeps a union-find of type variables and unifies
//! them along every flow, the way Hindley–Milner inference does. An object is
//! open — a field first written later is simply added — and two objects that
//! meet merge their fields. A field written under a name known only when the
//! program runs (`counts["$name"] = 1`) makes the object a map: one kind for
//! every value.
//!
//! What it cannot see is not guessed at: a module's answer, a field read by a
//! computed name, iterating a record's fields — those are `Any`, and `Any`
//! is never reported. A handler's field list is what arrives; its labels
//! (`fish ∈ { x ∈ Number }`) seed the kinds of what it receives.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::ast::{
    BinOp, EmitResult, EmitTarget, Expr, Field, FieldKey, Label, LoopOver, Program, Span, Stmt,
    UnOp, ValueKind,
};
use crate::diagnostics::{Diagnostic, Severity};
use crate::span::Origin;
use crate::types::Types;

type Var = usize;

#[derive(Debug, Clone)]
enum Shape {
    /// Nothing known yet — only ever null so far, or not reached.
    Unknown,
    /// Any kind on purpose (`∈ Any`), or not knowable here. Never reported.
    Any,
    /// Number, String or Boolean.
    Scalar(ValueKind),
    /// One of several types, born that way: a list written with a text and
    /// an element in it (`[name, icon]`) holds `String ∪ Object`. More of
    /// those types may join it later; another type is a change.
    Mixed(Vec<ValueKind>),
    /// A list whose every element is the one variable.
    Array(Var),
    /// An object: its fields so far, or — once a field is named at run time —
    /// a map whose every value is `map`.
    Object {
        fields: BTreeMap<String, Var>,
        map: Option<Var>,
    },
}

pub fn kind_warnings(program: &Program) -> Vec<Diagnostic> {
    let mut warnings = Vec::new();
    let mut suggestions = Vec::new();
    check_file(
        &program.statements,
        program.origin.as_ref(),
        &mut warnings,
        &mut suggestions,
        &Types::of(program),
    );
    warnings
}

/// The name `Suggestion::label` uses for a part nothing in the program
/// decides. Lowercase, so it can never be a kind or a class.
pub const UNKNOWN: &str = "_";

/// A label for a handler field that has none, as the inference sees it —
/// what `code check --write-types` writes (ticket 122, A.3).
pub struct Suggestion {
    /// The file the handler is in.
    pub origin: Option<Origin>,
    /// The field's declaration in that file.
    pub span: Option<Span>,
    pub handler: String,
    pub field: String,
    /// What the program shows, with `UNKNOWN` where it shows nothing.
    pub label: Option<Label>,
}

/// Where a `.field` read finds its field when the object is built the way
/// the program builds it (ticket 122, stage B): for the read at `expr` (by
/// address), the key list of an object literal that flows to it and the
/// field's position in that list. Codegen checks the object still has that
/// shape and reads the slot without a call.
#[derive(Default)]
pub struct Layouts {
    /// `.field` reads, by the read's address.
    pub fields: HashMap<usize, (Vec<String>, usize)>,
    /// `xs[i].x = v` writes: by the `SetAt`'s address and the step.
    pub parts: HashMap<(usize, usize), (Vec<String>, usize)>,
    /// The variable, `.field` and `[index]` reads whose value the program
    /// only ever makes a number (or null): their address. Codegen reads the number in
    /// line when the tag says so, and calls the runtime when it does not.
    pub numbers: std::collections::HashSet<usize>,
    /// Sites proved closed under ticket 126's internal-handler escape rule.
    /// This proves escape scope only, not backend operation support.
    pub closed_numeric: ClosedNumeric,
}

#[derive(Default)]
pub struct ClosedNumeric {
    pub lists: HashSet<usize>,
    pub records: HashSet<usize>,
}

/// Internal handler inputs and replies join the same storage group. Values
/// crossing a module, dynamic label or tracing boundary remain boxed.
pub fn closed_numeric_values(program: &Program) -> ClosedNumeric {
    field_layouts(program).closed_numeric
}

pub fn field_layouts(program: &Program) -> Layouts {
    field_layouts_with_boundaries(program, false)
}

/// Library exports, browser callbacks and compiled tracing expose values to
/// code outside this compilation. These builds conservatively keep boxes.
pub fn field_layouts_with_boundaries(program: &Program, exposed: bool) -> Layouts {
    let mut warnings = Vec::new();
    let mut suggestions = Vec::new();
    let mut layouts = Layouts::default();
    check_file_into(
        &program.statements,
        program.origin.as_ref(),
        &mut warnings,
        &mut suggestions,
        &mut layouts,
        &Types::of(program),
        false,
    );
    if exposed {
        layouts.closed_numeric = ClosedNumeric::default();
    }
    layouts
}

pub fn suggested_labels(program: &Program) -> Vec<Suggestion> {
    let mut warnings = Vec::new();
    let mut suggestions = Vec::new();
    check_file(
        &program.statements,
        program.origin.as_ref(),
        &mut warnings,
        &mut suggestions,
        &Types::of(program),
    );
    suggestions
}

fn check_file(
    statements: &[Stmt],
    origin: Option<&Origin>,
    warnings: &mut Vec<Diagnostic>,
    suggestions: &mut Vec<Suggestion>,
    defs: &Types,
) {
    let mut layouts = Layouts::default();
    check_file_into(
        statements,
        origin,
        warnings,
        suggestions,
        &mut layouts,
        defs,
        false,
    );
}

fn check_file_into(
    statements: &[Stmt],
    origin: Option<&Origin>,
    warnings: &mut Vec<Diagnostic>,
    suggestions: &mut Vec<Suggestion>,
    layouts: &mut Layouts,
    defs: &Types,
    imported: bool,
) {
    let mut infer = Infer {
        defs,
        type_vars: HashMap::new(),
        parent: Vec::new(),
        shapes: Vec::new(),
        labelled: Vec::new(),
        mixed: false,
        inputs: HashMap::new(),
        results: HashMap::new(),
        local: std::collections::HashSet::new(),
        scopes: vec![HashMap::new()],
        span: None,
        handler: None,
        origin,
        warnings,
        suggestions,
        layouts,
        literal_shapes: Vec::new(),
        field_sites: Vec::new(),
        part_sites: Vec::new(),
        read_sites: Vec::new(),
        storage_sites: Vec::new(),
        contains: Vec::new(),
        escapes: Vec::new(),
        comparisons: Vec::new(),
        unknown_dispatch: false,
    };
    for statement in statements {
        if let Stmt::HandlerDef { class_name, .. } = statement {
            infer.local.insert(class_name.clone());
        }
    }
    // A linked module can enter any handler and receive its reply. Local
    // literal sends still unify both sides, but these handler groups escape.
    if imported || external_code(statements) {
        for class in infer.local.clone() {
            let input = infer.input(&class);
            let result = infer.result(&class);
            infer.escapes.extend([input, result]);
        }
    }
    // A handler's labels say what it receives before any body sends it
    // something, wherever in the file the handler is written.
    for statement in statements {
        if let Stmt::HandlerDef {
            class_name, fields, ..
        } = statement
        {
            let input = infer.input(class_name);
            for field in fields {
                if let Some(label) = &field.annotation {
                    let var = infer.field(input, &field.field);
                    let labelled = infer.label(label);
                    infer.unify(var, labelled, &format!("field '{}'", field.field));
                }
            }
        }
    }
    // The top level first, handlers after: a handler sees its file's top
    // level whole, wherever in the file it is written.
    for statement in statements {
        if !matches!(statement, Stmt::HandlerDef { .. }) {
            infer.statement(statement);
        }
    }
    for statement in statements {
        if let Stmt::HandlerDef {
            class_name,
            fields,
            body,
        } = statement
        {
            infer.handler(class_name, fields, body);
        }
    }
    for statement in statements {
        if let Stmt::HandlerDef {
            class_name, fields, ..
        } = statement
        {
            let input = infer.input(class_name);
            for field in fields.iter().filter(|f| f.annotation.is_none()) {
                let var = infer.field(input, &field.field);
                let label = infer.label_of(var, &mut Vec::new());
                infer.suggestions.push(Suggestion {
                    origin: origin.cloned(),
                    span: field.span,
                    handler: class_name.clone(),
                    field: field.field.clone(),
                    label,
                });
            }
        }
    }
    // Each read's layout: the first shape built into its object's kind that
    // has the field. Another shape at run time just takes the slow path.
    let mut shapes: HashMap<Var, Vec<Vec<String>>> = HashMap::new();
    for (var, order) in std::mem::take(&mut infer.literal_shapes) {
        let root = infer.root(var);
        let known = shapes.entry(root).or_default();
        if !known.contains(&order) {
            known.push(order);
        }
    }
    let layout_of = |infer: &mut Infer, object: Var, field: &str| {
        let root = infer.root(object);
        shapes.get(&root).and_then(|candidates| {
            candidates.iter().find_map(|order| {
                order
                    .iter()
                    .position(|k| k == field)
                    .map(|at| (order.clone(), at))
            })
        })
    };
    for (site, object, field) in std::mem::take(&mut infer.field_sites) {
        if let Some(layout) = layout_of(&mut infer, object, &field) {
            infer.layouts.fields.insert(site, layout);
        }
    }
    for (site, object, field) in std::mem::take(&mut infer.part_sites) {
        if let Some(layout) = layout_of(&mut infer, object, &field) {
            infer.layouts.parts.insert(site, layout);
        }
    }
    for (site, read) in std::mem::take(&mut infer.read_sites) {
        if matches!(infer.shape(read), Shape::Scalar(ValueKind::Number)) {
            infer.layouts.numbers.insert(site);
        }
    }
    infer.finish_escape_analysis();
}

/// `Point { … }` — an object literal whose `_class` is written down.
/// The types a shape's values are of: none for `Unknown` and `Any`.
fn kinds_of(shape: &Shape) -> Vec<ValueKind> {
    match shape {
        Shape::Unknown | Shape::Any => Vec::new(),
        Shape::Scalar(kind) => vec![*kind],
        Shape::Mixed(kinds) => kinds.clone(),
        Shape::Array(_) => vec![ValueKind::Array],
        Shape::Object { .. } => vec![ValueKind::Object],
    }
}

fn external_code(statements: &[Stmt]) -> bool {
    statements.iter().any(|statement| match statement {
        Stmt::ImportNative { .. }
        | Stmt::LinkRuntime { .. }
        | Stmt::Link { .. }
        | Stmt::Import { .. } => true,
        Stmt::If { body, .. } | Stmt::Loop { body, .. } | Stmt::HandlerDef { body, .. } => {
            external_code(body)
        }
        _ => false,
    })
}

fn dynamic_label(label: &Label, defs: &Types, seen: &mut HashSet<String>) -> bool {
    match label {
        Label::Union(_) => true,
        Label::Named(name) if name == "Any" => true,
        Label::Named(name) if seen.insert(name.clone()) => defs
            .get(name)
            .is_some_and(|label| dynamic_label(label, defs, seen)),
        Label::ArrayOf(inner) | Label::ObjectOf(inner) => dynamic_label(inner, defs, seen),
        Label::Fields(fields) | Label::Open(fields) => fields
            .iter()
            .any(|(_, label)| dynamic_label(label, defs, seen)),
        _ => false,
    }
}

fn literal_class(expr: &Expr) -> Option<&str> {
    let Expr::Object(fields) = expr else {
        return None;
    };
    fields.iter().find_map(|(key, value)| match (key, value) {
        (FieldKey::Literal(name), Expr::Str(class)) if name == "_class" => Some(class.as_str()),
        _ => None,
    })
}

struct Infer<'a> {
    /// The program's `type`s (ticket 123).
    defs: &'a Types,
    /// The variable each `type` name stands for once reached, so a type
    /// that contains itself (`Element`) is one variable, not an endless one.
    type_vars: HashMap<String, Var>,
    parent: Vec<Var>,
    shapes: Vec<Shape>,
    /// Whether a variable's kind comes from a label. A value that breaks a
    /// label is the label check's to report (`diagnostics.rs`), as an error;
    /// this reports only kinds no label spoke for.
    labelled: Vec<bool>,
    /// Set while the elements of one list literal meet: a list written with
    /// a text and an element in it is born of several types, which is no
    /// value changing type — its elements are `Shape::Mixed`.
    mixed: bool,
    /// What each of this file's handlers receives: the particles emitted to
    /// it flow in here. A class name groups nothing by itself — two
    /// `Result { … }` literals that never meet are two values.
    inputs: HashMap<String, Var>,
    /// What each handler answers: its `return`s flow in here, and `get`
    /// reads it out.
    results: HashMap<String, Var>,
    /// The classes this file has a handler for.
    local: std::collections::HashSet<String>,
    scopes: Vec<HashMap<String, Var>>,
    /// The statement being looked at, for the warning's location.
    span: Option<Span>,
    handler: Option<String>,
    origin: Option<&'a Origin>,
    warnings: &'a mut Vec<Diagnostic>,
    suggestions: &'a mut Vec<Suggestion>,
    layouts: &'a mut Layouts,
    /// Every object literal whose keys are all written down: its variable and
    /// its keys in order — the shapes the program builds.
    literal_shapes: Vec<(Var, Vec<String>)>,
    /// Every `.field` read: its address, the object's variable, the field.
    field_sites: Vec<(usize, Var, String)>,
    /// Every named step of a part write: (address, step), object, field.
    part_sites: Vec<((usize, usize), Var, String)>,
    /// Every `.field` and `[index]` read: its address and what it reads.
    read_sites: Vec<(usize, Var)>,
    storage_sites: Vec<(usize, Var)>,
    // Keep original containment edges even if a parent later becomes Any.
    contains: Vec<(Var, Var)>,
    escapes: Vec<Var>,
    comparisons: Vec<(Var, Var)>,
    unknown_dispatch: bool,
}

impl Infer<'_> {
    fn fresh(&mut self, shape: Shape) -> Var {
        let var = self.parent.len();
        match &shape {
            Shape::Array(element) => self.contains.push((var, *element)),
            Shape::Object { fields, map } => {
                self.contains
                    .extend(fields.values().map(|child| (var, *child)));
                if let Some(child) = map {
                    self.contains.push((var, *child));
                }
            }
            _ => {}
        }
        self.parent.push(var);
        self.shapes.push(shape);
        self.labelled.push(false);
        var
    }

    fn finish_escape_analysis(&mut self) {
        let mut children: HashMap<Var, Vec<Var>> = HashMap::new();
        for (parent, child) in std::mem::take(&mut self.contains) {
            let parent = self.root(parent);
            let child = self.root(child);
            children.entry(parent).or_default().push(child);
        }
        let mut todo = std::mem::take(&mut self.escapes);
        if self.local.contains(crate::flows::HANDLER) {
            // Trace can observe an interaction's particles and replies,
            // including nested values. Keep the whole compilation boxed.
            todo.extend(0..self.parent.len());
        }
        if self.unknown_dispatch {
            todo.extend(self.inputs.values().copied());
            todo.extend(self.results.values().copied());
        }
        for var in 0..self.parent.len() {
            if matches!(self.shape(var), Shape::Any | Shape::Mixed(_)) {
                todo.push(var);
            }
        }
        for (left, right) in std::mem::take(&mut self.comparisons) {
            if self.root(left) != self.root(right) {
                todo.extend([left, right]);
            }
        }
        let mut escaped = HashSet::new();
        while let Some(var) = todo.pop() {
            let root = self.root(var);
            if escaped.insert(root) {
                if let Some(parts) = children.get(&root) {
                    todo.extend(parts);
                }
            }
        }
        for (site, var) in std::mem::take(&mut self.storage_sites) {
            let root = self.root(var);
            if escaped.contains(&root) {
                continue;
            }
            match self.shape(root) {
                Shape::Array(element)
                    if matches!(self.shape(element), Shape::Scalar(ValueKind::Number)) =>
                {
                    self.layouts.closed_numeric.lists.insert(site);
                }
                Shape::Object { fields, map: None }
                    if !fields.is_empty()
                        && fields.values().all(|field| {
                            matches!(self.shape(*field), Shape::Scalar(ValueKind::Number))
                        }) =>
                {
                    self.layouts.closed_numeric.records.insert(site);
                }
                _ => {}
            }
        }
    }

    fn root(&mut self, var: Var) -> Var {
        let mut at = var;
        while self.parent[at] != at {
            self.parent[at] = self.parent[self.parent[at]];
            at = self.parent[at];
        }
        at
    }

    fn shape(&mut self, var: Var) -> Shape {
        let root = self.root(var);
        self.shapes[root].clone()
    }

    fn scalar(&mut self, kind: ValueKind) -> Var {
        self.fresh(Shape::Scalar(kind))
    }

    /// Makes `a` and `b` one kind. `what` names the thing whose kind is
    /// being fixed, for the warning: `'count'`, `field 'pos.x'`.
    fn unify(&mut self, a: Var, b: Var, what: &str) {
        let (ra, rb) = (self.root(a), self.root(b));
        if ra == rb {
            return;
        }
        let (sa, sb) = (self.shapes[ra].clone(), self.shapes[rb].clone());
        // `b` is folded into `a`; the merged shape is written to `a`.
        self.parent[rb] = ra;
        let labelled = self.labelled[ra] || self.labelled[rb];
        self.labelled[ra] = labelled;
        let merged = match (sa, sb) {
            (Shape::Unknown, other) | (other, Shape::Unknown) => other,
            (Shape::Any, _) | (_, Shape::Any) => Shape::Any,
            (Shape::Scalar(x), Shape::Scalar(y)) if x == y => Shape::Scalar(x),
            (Shape::Array(x), Shape::Array(y)) => {
                self.unify(x, y, &format!("an element of {what}"));
                Shape::Array(x)
            }
            (
                Shape::Object {
                    fields: mut fa,
                    map: ma,
                },
                Shape::Object {
                    fields: fb,
                    map: mb,
                },
            ) => {
                for (name, vb) in fb {
                    match fa.get(&name) {
                        Some(&va) => self.unify(va, vb, &field_of(what, &name)),
                        None => {
                            fa.insert(name, vb);
                        }
                    }
                }
                let map = match (ma, mb) {
                    (Some(x), Some(y)) => {
                        self.unify(x, y, &format!("a value of {what}"));
                        Some(x)
                    }
                    (x, y) => x.or(y),
                };
                if let Some(values) = map {
                    for var in fa.values().copied().collect::<Vec<_>>() {
                        self.unify(values, var, &format!("a value of {what}"));
                    }
                }
                Shape::Object { fields: fa, map }
            }
            (sa, sb) => {
                let (ka, kb) = (kinds_of(&sa), kinds_of(&sb));
                let joined = {
                    let mut all = ka.clone();
                    for kind in &kb {
                        if !all.contains(kind) {
                            all.push(*kind);
                        }
                    }
                    all
                };
                if self.mixed {
                    Shape::Mixed(joined)
                } else if joined.len() == ka.len() && matches!(sa, Shape::Mixed(_)) {
                    sa
                } else if joined.len() == kb.len() && matches!(sb, Shape::Mixed(_)) {
                    sb
                } else {
                    if !labelled {
                        self.report(what, &sa, &sb);
                    }
                    Shape::Any
                }
            }
        };
        self.shapes[ra] = merged;
    }

    fn report(&mut self, what: &str, was: &Shape, now: &Shape) {
        let name = |shape: &Shape| match shape {
            Shape::Scalar(kind) => kind.name().to_string(),
            Shape::Array(_) => "Array".to_string(),
            Shape::Object { .. } => "Object".to_string(),
            Shape::Mixed(kinds) => kinds
                .iter()
                .map(|kind| kind.name())
                .collect::<Vec<_>>()
                .join(" ∪ "),
            Shape::Any | Shape::Unknown => "Any".to_string(),
        };
        let (was, now) = (name(was), name(now));
        self.warnings.push(Diagnostic {
            code: "type-change",
            severity: Severity::Error,
            message: format!(
                "{what} holds {} and is given {} here — a value keeps the type it first \
                 held (null aside); use another name, or a label (`∈ Any`, `∈ String ∪ \
                 Element`) for one that is several types on purpose",
                article(&was),
                article(&now)
            ),
            handler: self.handler.clone(),
            particle: None,
            target: None,
            field: None,
            expected: Some(was),
            actual: Some(now),
            suggestion: None,
            span: self.span,
            origin: self.origin.cloned(),
        });
    }

    fn input(&mut self, class: &str) -> Var {
        if let Some(&var) = self.inputs.get(class) {
            return var;
        }
        let var = self.fresh(Shape::Unknown);
        self.inputs.insert(class.to_string(), var);
        var
    }

    fn result(&mut self, class: &str) -> Var {
        if let Some(&var) = self.results.get(class) {
            return var;
        }
        let var = self.fresh(Shape::Unknown);
        self.results.insert(class.to_string(), var);
        var
    }

    /// The variable for field `name` of `object`, made if it is not there.
    fn field(&mut self, object: Var, name: &str) -> Var {
        let root = self.root(object);
        match self.shapes[root].clone() {
            Shape::Unknown => {
                let field = self.fresh(Shape::Unknown);
                self.contains.push((root, field));
                self.shapes[root] = Shape::Object {
                    fields: BTreeMap::from([(name.to_string(), field)]),
                    map: None,
                };
                field
            }
            Shape::Object { mut fields, map } => {
                if let Some(&field) = fields.get(name) {
                    return field;
                }
                let field = match map {
                    Some(values) => values,
                    None => self.fresh(Shape::Unknown),
                };
                fields.insert(name.to_string(), field);
                self.contains.push((root, field));
                self.shapes[root] = Shape::Object { fields, map };
                field
            }
            _ => self.fresh(Shape::Any),
        }
    }

    /// The one variable every value of a map shares; `object` becomes a map.
    fn map_values(&mut self, object: Var, what: &str) -> Var {
        let root = self.root(object);
        match self.shapes[root].clone() {
            Shape::Unknown => {
                let values = self.fresh(Shape::Unknown);
                self.contains.push((root, values));
                self.shapes[root] = Shape::Object {
                    fields: BTreeMap::new(),
                    map: Some(values),
                };
                values
            }
            Shape::Object { fields, map } => {
                let values = match map {
                    Some(values) => values,
                    None => self.fresh(Shape::Unknown),
                };
                self.contains.push((root, values));
                self.shapes[root] = Shape::Object {
                    fields: fields.clone(),
                    map: Some(values),
                };
                for var in fields.values() {
                    self.unify(values, *var, &format!("a value of {what}"));
                }
                values
            }
            _ => self.fresh(Shape::Any),
        }
    }

    fn element(&mut self, list: Var) -> Var {
        let root = self.root(list);
        match self.shapes[root].clone() {
            Shape::Unknown => {
                let element = self.fresh(Shape::Unknown);
                self.shapes[root] = Shape::Array(element);
                self.contains.push((root, element));
                element
            }
            Shape::Array(element) => element,
            Shape::Scalar(ValueKind::String) => self.scalar(ValueKind::String),
            _ => self.fresh(Shape::Any),
        }
    }

    fn label(&mut self, label: &Label) -> Var {
        let var = self.label_shape(label);
        let root = self.root(var);
        self.labelled[root] = true;
        if dynamic_label(label, self.defs, &mut HashSet::new()) {
            self.escapes.push(var);
        }
        var
    }

    fn label_shape(&mut self, label: &Label) -> Var {
        match label {
            Label::Named(name) => match name.as_str() {
                "Any" => self.fresh(Shape::Any),
                "Null" => self.fresh(Shape::Unknown),
                "Array" => {
                    let element = self.fresh(Shape::Unknown);
                    self.fresh(Shape::Array(element))
                }
                "Object" => self.fresh(Shape::Unknown),
                other if self.defs.get(other).is_some() => {
                    if let Some(&var) = self.type_vars.get(other) {
                        return var;
                    }
                    let var = self.fresh(Shape::Unknown);
                    self.type_vars.insert(other.to_string(), var);
                    let def = self.defs.get(other).cloned().expect("checked above");
                    let shape = self.label(&def);
                    self.unify(var, shape, &format!("type '{other}'"));
                    var
                }
                other => match ValueKind::parse(other) {
                    Some(kind) => self.scalar(kind),
                    // A particle this file handles is what that handler
                    // receives; any other is an object, fields unknown.
                    None if self.local.contains(other) => self.input(other),
                    None => self.fresh(Shape::Unknown),
                },
            },
            Label::Fields(fields) | Label::Open(fields) => {
                let mut vars = BTreeMap::new();
                for (name, field) in fields {
                    let var = self.label(field);
                    vars.insert(name.clone(), var);
                }
                self.fresh(Shape::Object {
                    fields: vars,
                    map: None,
                })
            }
            Label::ArrayOf(element) => {
                let element = self.label(element);
                self.fresh(Shape::Array(element))
            }
            Label::ObjectOf(values) => {
                let values = self.label(values);
                self.fresh(Shape::Object {
                    fields: BTreeMap::new(),
                    map: Some(values),
                })
            }
            Label::Value(value) => self.scalar(value.kind()),
            // One type throughout (`"red" ∪ "green"`, two object labels):
            // the members as one; several types: `Any`, until a value's
            // type is something the compiler keeps per value.
            Label::Union(members) => match self.defs.kind_of(label) {
                Some(_) => {
                    let var = self.label(&members[0]);
                    for member in &members[1..] {
                        let other = self.label(member);
                        self.unify(var, other, "a member of a union");
                    }
                    var
                }
                None => self.fresh(Shape::Any),
            },
        }
    }

    /// The label that says what `var` holds, to the last field. A part
    /// nothing in the program decides (only ever null, or never reached) is
    /// `UNKNOWN`, for `write_types` to fill from what the fixtures saw; a
    /// shape that contains itself is `Any` where it repeats.
    fn label_of(&mut self, var: Var, visiting: &mut Vec<Var>) -> Option<Label> {
        let root = self.root(var);
        if visiting.contains(&root) || visiting.len() > 12 {
            return Some(Label::Named("Any".to_string()));
        }
        visiting.push(root);
        let any = || Label::Named(UNKNOWN.to_string());
        let label = match self.shape(root) {
            Shape::Unknown => Some(any()),
            Shape::Any => Some(Label::Named("Any".to_string())),
            Shape::Scalar(kind) => Some(Label::Named(kind.name().to_string())),
            Shape::Mixed(kinds) => Some(Label::Union(
                kinds
                    .iter()
                    .map(|kind| Label::Named(kind.name().to_string()))
                    .collect(),
            )),
            Shape::Array(element) => Some(Label::ArrayOf(Box::new(
                self.label_of(element, visiting).unwrap_or_else(any),
            ))),
            Shape::Object {
                map: Some(values), ..
            } => Some(Label::ObjectOf(Box::new(
                self.label_of(values, visiting).unwrap_or_else(any),
            ))),
            Shape::Object { fields, map: None } => {
                let mut labelled = Vec::new();
                for (name, field) in fields {
                    if name == "_class" {
                        continue;
                    }
                    let label = self.label_of(field, visiting).unwrap_or_else(any);
                    labelled.push((name, label));
                }
                Some(Label::Fields(labelled))
            }
        };
        visiting.pop();
        label
    }

    fn lookup(&self, name: &str) -> Option<Var> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    fn bind(&mut self, name: &str, var: Var) {
        self.scopes
            .last_mut()
            .expect("a scope")
            .insert(name.to_string(), var);
    }

    /// `name` takes `value`: the binding's kind if it has one, its first
    /// otherwise.
    fn assign(&mut self, name: &str, value: Var) {
        match self.lookup(name) {
            Some(var) => self.unify(var, value, &format!("'{name}'")),
            None => self.bind(name, value),
        }
    }

    fn handler(&mut self, class_name: &str, fields: &[Field], body: &[Stmt]) {
        self.handler = Some(class_name.to_string());
        let class = self.input(class_name);
        self.scopes.push(HashMap::new());
        for field in fields {
            let var = self.field(class, &field.field);
            if let Some(label) = &field.annotation {
                let labelled = self.label(label);
                self.unify(var, labelled, &format!("field '{}'", field.field));
            }
            self.bind(&field.name, var);
        }
        self.statements(body);
        self.scopes.pop();
        self.handler = None;
    }

    fn statements(&mut self, statements: &[Stmt]) {
        for statement in statements {
            self.statement(statement);
        }
    }

    fn statement(&mut self, statement: &Stmt) {
        // A statement with no span of its own (an `assert`) is not said to
        // be where the one before it was.
        self.span = None;
        match statement {
            Stmt::Assign {
                name,
                annotation,
                value,
                span,
            } => {
                self.span = *span;
                let value = self.expr(value);
                if let Some(label) = annotation {
                    let labelled = self.label(label);
                    self.unify(labelled, value, &format!("'{name}'"));
                    self.assign(name, labelled);
                } else {
                    self.assign(name, value);
                }
            }
            Stmt::Assert(expr) => {
                self.expr(expr);
            }
            Stmt::If { condition, body } => {
                self.expr(condition);
                self.scopes.push(HashMap::new());
                self.statements(body);
                self.scopes.pop();
            }
            Stmt::Loop { over, body } => {
                self.scopes.push(HashMap::new());
                if let Some(over) = over {
                    self.loop_over(over);
                }
                self.statements(body);
                self.scopes.pop();
            }
            Stmt::Emit {
                particle,
                target,
                result,
                span,
            } => {
                self.span = *span;
                let sent = self.expr(particle);
                let local = match target {
                    EmitTarget::This => literal_class(particle)
                        .filter(|class| self.local.contains(*class))
                        .map(str::to_string),
                    _ => None,
                };
                if local.is_none() {
                    self.escapes.push(sent);
                    if matches!(target, EmitTarget::This | EmitTarget::Base) {
                        self.unknown_dispatch = true;
                    }
                }
                let answer = match (&local, target) {
                    (Some(class), _) => {
                        let input = self.input(class);
                        self.unify(input, sent, &format!("a {class} sent"));
                        self.result(class)
                    }
                    (None, EmitTarget::This | EmitTarget::Base) => self.fresh(Shape::Unknown),
                    (None, EmitTarget::Core | EmitTarget::Module(_)) => self.fresh(Shape::Any),
                };
                match result {
                    Some(EmitResult::Whole(name)) => self.assign(name, answer),
                    Some(EmitResult::Fields(fields)) => {
                        for field in fields {
                            let var = self.field(answer, &field.field);
                            if let Some(label) = &field.annotation {
                                let labelled = self.label(label);
                                self.unify(var, labelled, &format!("field '{}'", field.field));
                            }
                            self.assign(&field.name, var);
                        }
                    }
                    None => {}
                }
            }
            Stmt::Return { value, span } => {
                self.span = *span;
                let value = self.expr(value);
                if let Some(handler) = self.handler.clone() {
                    let result = self.result(&handler);
                    self.unify(result, value, &format!("what {handler} answers"));
                }
            }
            Stmt::Import { body, origin, .. } => check_file_into(
                body,
                Some(origin),
                self.warnings,
                self.suggestions,
                self.layouts,
                self.defs,
                true,
            ),
            Stmt::Link { alias, .. } => {
                if let Some(alias) = alias {
                    let any = self.fresh(Shape::Any);
                    self.bind(alias, any);
                }
            }
            Stmt::ImportNative { alias, .. } | Stmt::LinkRuntime { alias, .. } => {
                let any = self.fresh(Shape::Any);
                self.bind(alias, any);
            }
            Stmt::Unlink(expr) => {
                self.expr(expr);
            }
            Stmt::HandlerDef { .. } | Stmt::Continue | Stmt::Break | Stmt::TypeDef { .. } => {}
        }
    }

    fn loop_over(&mut self, over: &LoopOver) {
        let iterable = self.expr(&over.iterable);
        let (key, value) = match self.shape(iterable) {
            Shape::Array(element) => (self.scalar(ValueKind::Number), element),
            Shape::Object {
                map: Some(values), ..
            } => (self.scalar(ValueKind::String), values),
            // A record's fields are of many kinds; walking them is dynamic.
            Shape::Object { .. } => (self.scalar(ValueKind::String), self.fresh(Shape::Any)),
            _ => (self.fresh(Shape::Unknown), self.fresh(Shape::Unknown)),
        };
        if let Some(name) = &over.key {
            self.bind(name, key);
        }
        self.bind(&over.value, value);
    }

    fn expr(&mut self, expr: &Expr) -> Var {
        let var = match expr {
            Expr::Number(_) | Expr::LengthOf(_) => {
                if let Expr::LengthOf(inner) = expr {
                    self.expr(inner);
                }
                self.scalar(ValueKind::Number)
            }
            Expr::Str(_) => self.scalar(ValueKind::String),
            Expr::Interpolated(parts) => {
                for part in parts {
                    let rendered = self.expr(part);
                    self.escapes.push(rendered);
                }
                self.scalar(ValueKind::String)
            }
            Expr::Bool(_) => self.scalar(ValueKind::Boolean),
            Expr::Null => self.fresh(Shape::Unknown),
            Expr::Ident(name) => match self.lookup(name) {
                Some(var) => {
                    self.read_sites.push((expr as *const Expr as usize, var));
                    var
                }
                None => self.fresh(Shape::Any),
            },
            Expr::Array(items) => {
                let element = self.fresh(Shape::Unknown);
                let items: Vec<Var> = items.iter().map(|item| self.expr(item)).collect();
                let mixed = std::mem::replace(&mut self.mixed, true);
                for item in items {
                    self.unify(element, item, "an element of a list");
                }
                self.mixed = mixed;
                self.fresh(Shape::Array(element))
            }
            Expr::Object(fields) => self.object(fields),
            Expr::Field(object, name) => {
                let object = self.expr(object);
                self.field_sites
                    .push((expr as *const Expr as usize, object, name.clone()));
                let read = self.field(object, name);
                self.read_sites.push((expr as *const Expr as usize, read));
                read
            }
            Expr::Index(container, index) => {
                let read = self.index(container, index);
                self.read_sites.push((expr as *const Expr as usize, read));
                read
            }
            Expr::Slice { value, from, to } => {
                self.expr(from);
                self.expr(to);
                self.expr(value)
            }
            Expr::SetAt { .. } => self.set_at(expr),
            Expr::Unary(UnOp::Neg, inner) => {
                self.expr(inner);
                self.scalar(ValueKind::Number)
            }
            Expr::Unary(UnOp::Not, inner) | Expr::Is(inner, _) => {
                self.expr(inner);
                self.scalar(ValueKind::Boolean)
            }
            Expr::Binary(left, op, right) => self.binary(left, *op, right),
        };
        self.storage_sites.push((expr as *const Expr as usize, var));
        var
    }

    fn index(&mut self, container_expr: &Expr, index: &Expr) -> Var {
        let container = self.expr(container_expr);
        self.expr(index);
        match (self.shape(container), index) {
            (Shape::Array(element), _) => element,
            (Shape::Scalar(ValueKind::String), _) => self.scalar(ValueKind::String),
            (
                Shape::Object {
                    map: Some(values), ..
                },
                _,
            ) => values,
            (Shape::Object { .. } | Shape::Unknown, Expr::Str(name)) => self.field(container, name),
            // A text is indexed by number too (`s[0]`), so an index alone does
            // not make an unknown value a list.
            (Shape::Unknown, _) => self.fresh(Shape::Any),
            _ => self.fresh(Shape::Any),
        }
    }

    fn set_at(&mut self, expr: &Expr) -> Var {
        let Expr::SetAt { base, path, value } = expr else {
            unreachable!("set_at is given a SetAt");
        };
        let base = self.expr(base);
        let mut at = base;
        let mut spelled = String::new();
        for (step, key) in path.iter().enumerate() {
            let key_var = self.expr(key);
            at = match key {
                Expr::Str(name) => {
                    spelled.push('.');
                    spelled.push_str(name);
                    self.part_sites
                        .push(((expr as *const Expr as usize, step), at, name.clone()));
                    self.field(at, name)
                }
                _ => match self.shape(key_var) {
                    Shape::Scalar(ValueKind::Number) => {
                        spelled.push_str("[…]");
                        self.element(at)
                    }
                    _ => {
                        spelled.push_str("[…]");
                        self.map_values(at, "an object")
                    }
                },
            };
        }
        let value = self.expr(value);
        let what = match base_name(expr) {
            Some(name) => format!("'{name}{spelled}'"),
            None => format!("'{spelled}'"),
        };
        self.unify(at, value, &what);
        base
    }

    fn object(&mut self, fields: &[(FieldKey, Expr)]) -> Var {
        let mut known = BTreeMap::new();
        let mut map = None;
        let order: Option<Vec<String>> = fields
            .iter()
            .map(|(key, _)| match key {
                FieldKey::Literal(name) => Some(name.clone()),
                FieldKey::Computed(_) => None,
            })
            .collect();
        for (key, value) in fields {
            let value = self.expr(value);
            match key {
                FieldKey::Literal(name) => {
                    known.insert(name.clone(), value);
                }
                FieldKey::Computed(name) => {
                    self.expr(name);
                    let values = *map.get_or_insert_with(|| self.fresh(Shape::Unknown));
                    // Written together, its values are born of whatever
                    // they hold — as a list written with several types is.
                    let mixed = std::mem::replace(&mut self.mixed, true);
                    self.unify(values, value, "a value of an object");
                    self.mixed = mixed;
                }
            }
        }
        let var = self.fresh(Shape::Object { fields: known, map });
        if let Some(order) = order.filter(|order| !order.is_empty()) {
            self.literal_shapes.push((var, order));
        }
        var
    }

    fn binary(&mut self, left: &Expr, op: BinOp, right: &Expr) -> Var {
        let l = self.expr(left);
        let r = self.expr(right);
        if matches!(op, BinOp::Eq | BinOp::Ne) {
            self.comparisons.push((l, r));
        }
        // A list written right here and joined (`[name] + controls`, `[1] +
        // true`, `"x" + [1, 2]`) is a new list, born of whatever both sides
        // hold — as a list written with several types is. `xs + [x]` is
        // `xs` growing, and keeps its type.
        let written = matches!(left, Expr::Array(_))
            || (matches!(right, Expr::Array(_)) && !matches!(self.shape(l), Shape::Array(_)));
        let mixed = self.mixed;
        self.mixed = mixed || written;
        let var = self.binary_of(left, op, right, l, r);
        self.mixed = mixed;
        var
    }

    fn binary_of(&mut self, left: &Expr, op: BinOp, right: &Expr, l: Var, r: Var) -> Var {
        match op {
            BinOp::Add => match (self.shape(l), self.shape(r)) {
                // `xs + ys` joins; `xs + x` appends; `x + xs` prepends. A side
                // not known yet next to a list is taken to be a list when it
                // is on the left (`stones += [ … ]`), and left alone on the
                // right, where it could be either.
                (Shape::Array(_), Shape::Array(_)) | (Shape::Unknown, Shape::Array(_)) => {
                    self.unify(l, r, &list_named(left));
                    l
                }
                (Shape::Array(_), Shape::Unknown) => l,
                (Shape::Array(element), _) => {
                    self.unify(element, r, &list_named(left));
                    l
                }
                (_, Shape::Array(element)) => {
                    self.unify(element, l, &list_named(right));
                    r
                }
                // A text and anything but a list is a text, as the
                // interpreter adds them; a list with a text appends it
                // (above).
                (Shape::Scalar(ValueKind::String), _) | (_, Shape::Scalar(ValueKind::String)) => {
                    self.scalar(ValueKind::String)
                }
                (Shape::Object { .. }, Shape::Object { .. }) => {
                    let what = match left {
                        Expr::Ident(name) => format!("'{name}'"),
                        _ => "an object".to_string(),
                    };
                    self.unify(l, r, &what);
                    l
                }
                (Shape::Scalar(ValueKind::Number), Shape::Scalar(ValueKind::Number)) => {
                    self.scalar(ValueKind::Number)
                }
                _ => self.fresh(Shape::Unknown),
            },
            BinOp::Sub | BinOp::Mul | BinOp::Div => self.scalar(ValueKind::Number),
            BinOp::Eq
            | BinOp::Ne
            | BinOp::Lt
            | BinOp::Gt
            | BinOp::Le
            | BinOp::Ge
            | BinOp::And
            | BinOp::Or => self.scalar(ValueKind::Boolean),
        }
    }
}

fn field_of(what: &str, name: &str) -> String {
    match what.strip_prefix('\'').and_then(|w| w.strip_suffix('\'')) {
        Some(inner) => format!("'{inner}.{name}'"),
        None => format!("field '{name}' of {what}"),
    }
}

fn list_named(list: &Expr) -> String {
    match list {
        Expr::Ident(name) => format!("an element of '{name}'"),
        _ => "an element of a list".to_string(),
    }
}

fn base_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::SetAt { base, .. } => match &**base {
            Expr::Ident(name) => Some(name),
            _ => None,
        },
        _ => None,
    }
}

fn article(kind: &str) -> String {
    match kind {
        _ if kind.contains('∪') => kind.to_string(),
        "Array" | "Object" | "Any" => format!("an {kind}"),
        _ => format!("a {kind}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer, parser};

    fn parsed(source: &str) -> Program {
        parser::parse(&lexer::tokenize(source).expect("tokenize")).expect("parse")
    }

    fn first_list_is_closed(source: &str) -> bool {
        let program = parsed(source);
        let Stmt::Assign { value, .. } = &program.statements[0] else {
            panic!("list assignment first")
        };
        closed_numeric_values(&program)
            .lists
            .contains(&(value as *const Expr as usize))
    }

    #[test]
    fn closed_lists_allow_scalar_reads_but_no_boundary_or_dynamic_flow() {
        assert!(first_list_is_closed(
            "xs = [1, 2]\nanswer = xs[0] + xs[1]\nassert answer = 3\n"
        ));
        assert!(first_list_is_closed(
            "xs = [1, 2]\nemit Answer { value = xs[0] } to core\n"
        ));
        for suffix in [
            "alias = xs\nemit Draw { value = alias } to canvas\n",
            "link \"external.so\" as external\nRead {} => return Result { value = xs }\n",
            "Read {} => return Result { value = xs }\nrequest = Read {}\nemit request to this\n",
            "anything ∈ Any = xs\n",
            "anything ∈ (Array of Number) ∪ String = xs\n",
            "container = { value = xs }\nanything ∈ Any = container\n",
            "container ∈ Any = null\ncontainer = { value = xs }\n",
            "other = [1, 2]\nassert xs = other\n",
            "emit Trace { value = xs } to console\n",
            "container = [xs, \"mixed\"]\n",
        ] {
            assert!(
                !first_list_is_closed(&format!("xs = [1, 2]\n{suffix}")),
                "{suffix}"
            );
        }
    }

    #[test]
    fn internal_handler_chains_join_inputs_and_replies() {
        assert!(first_list_is_closed("xs = [1, 2]\nFirst { value } =>\n    emit Second { value = value } to this get answer\n    return answer\nSecond { value } => return Result { value = value }\nemit First { value = xs } to this get answer\nassert answer.value[0] = 1\n"));
    }

    #[test]
    fn every_external_boundary_keeps_the_group_boxed() {
        for suffix in [
            "emit Draw { value = xs } to outside\n",
            "link \"external.a\" as outside\nemit Draw { value = xs } to outside\n",
            "link \"external.so\" as outside\nemit Draw { value = xs } to outside\n",
            "link \"hosted\" as outside\nemit Draw { value = xs } to outside\n",
            "link \"browser.a\" as outside\nemit Draw { value = xs } to outside\n",
            "emit Fetch {} to outside get reply\nxs = reply.value\n",
            "Trace { steps } => return null\n",
            "link \"external.so\" as outside\nRead {} => return Result { value = xs }\n",
        ] {
            assert!(
                !first_list_is_closed(&format!("xs = [1, 2]\n{suffix}")),
                "{suffix}"
            );
        }
        let program = parsed("xs = [1, 2]\n");
        assert!(
            field_layouts_with_boundaries(&program, true)
                .closed_numeric
                .lists
                .is_empty(),
            "code trace / exported builds"
        );
    }

    #[test]
    fn dynamic_labels_are_followed_through_named_types() {
        let program = parsed("type Values = (Array of Number) ∪ (Array of Number)\nxs = [1, 2]\nlabelled ∈ Values = xs\n");
        assert!(closed_numeric_values(&program).lists.is_empty());
    }

    #[test]
    fn numeric_records_need_the_same_closed_scope() {
        let local = parsed("p = { x = 1, y = 2 }\nanswer = p.x + p.y\nassert answer = 3\n");
        assert!(!closed_numeric_values(&local).records.is_empty());
        let sent = parsed("p = { x = 1, y = 2 }\nemit Draw { point = p } to canvas\n");
        assert!(closed_numeric_values(&sent).records.is_empty());
        let mixed = parsed("p = { x = 1, name = \"point\" }\n");
        assert!(closed_numeric_values(&mixed).records.is_empty());
    }

    #[test]
    fn internal_handler_scope_keeps_spectral_lists_and_points_records() {
        let spectral = parsed(include_str!("../benchmarks/workloads/spectral_norm.code"));
        let spectral = closed_numeric_values(&spectral);
        assert!(
            !spectral.lists.is_empty(),
            "internal Times inputs and Vec replies are inside the group"
        );
        let points = parsed(include_str!("../benchmarks/workloads/points.code"));
        let points = closed_numeric_values(&points);
        assert!(
            !points.records.is_empty(),
            "points records stay inside the program"
        );
        eprintln!(
            "closed sites: spectral lists {}, points records {}",
            spectral.lists.len(),
            points.records.len()
        );
    }

    fn warnings(source: &str) -> Vec<String> {
        let lexed = lexer::tokenize(source).expect("tokenize");
        let program = parser::parse(&lexed).expect("parse");
        kind_warnings(&program)
            .into_iter()
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn a_variable_keeps_its_first_kind() {
        let found = warnings("count = 1\ncount = 2\ncount = \"three\"\n");
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("'count' holds a Number and is given a String"));
    }

    #[test]
    fn a_list_born_mixed_holds_its_types() {
        assert!(warnings("x = [1, \"two\"]\nx += [3]\nx += [\"four\"]\n").is_empty());
        let late = warnings("x = [1, \"two\"]\nx += [true]\n");
        assert_eq!(late.len(), 1, "{late:?}");
        assert!(late[0].contains("Number ∪ String"), "{late:?}");
    }

    #[test]
    fn an_empty_list_takes_its_type_from_what_joins_it() {
        assert!(warnings("x = []\nx += [1]\nx += [2]\n").is_empty());
        assert_eq!(warnings("x = []\nx += [1]\nx += [\"two\"]\n").len(), 1);
        assert!(
            warnings("x ∈ Array of (Number ∪ String) = []\nx += [1]\nx += [\"two\"]\n").is_empty()
        );
    }

    #[test]
    fn a_text_added_to_a_list_is_an_element() {
        assert!(warnings("wrong = []\nwrong += \"bad key\"\nwrong += \"bad value\"\n").is_empty());
        assert!(warnings("names = [\"a\"]\nnames = \"b\" + names\n").is_empty());
    }

    #[test]
    fn a_list_written_and_joined_is_born_mixed() {
        assert!(warnings("x = [1] + true\ny = \"a\" + [1, 2]\n").is_empty());
        assert_eq!(warnings("xs = [1]\nxs += true\n").len(), 1);
    }

    #[test]
    fn null_is_not_a_kind() {
        assert!(warnings("id = null\nid = 5\nid = null\nname = \"\"\n").is_empty());
    }

    #[test]
    fn a_field_keeps_its_first_kind_however_it_is_written() {
        let found = warnings(
            "person = { name = \"Ali\" }\nperson.age = 24\nperson = person + { age = 25 }\nperson.age = \"old\"\n",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("'person.age' holds a Number and is given a String"));
    }

    #[test]
    fn values_that_meet_share_kinds() {
        let found =
            warnings("a = { x = 1 }\nb = a\nb.x = \"one\"\nxs = [{ y = 1 }]\nxs += { y = true }\n");
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].contains("'b.x'"), "{}", found[0]);
        assert!(found[1].contains("an element of 'xs'"), "{}", found[1]);
    }

    #[test]
    fn a_list_written_with_mixed_kinds_is_born_that_way() {
        // A text and an element side by side is what a UI tree is; until
        // ticket 123 gives it a union, its elements are `Any`.
        assert!(warnings(
            "page = { tag = \"p\", children = [\"Hi \", { tag = \"b\", children = [\"you\"] }] }\n"
        )
        .is_empty());
        // A list that was one kind still keeps it.
        let found = warnings("xs = [1, 2]\nxs += [\"three\"]\n");
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn a_text_is_indexed_by_number_too() {
        assert!(warnings(
            "S { s ∈ String } =>\n    t = s\n    if t[0] = \"T\", t = \"Add time\"\n    return Ok {}\n"
        )
        .is_empty());
    }

    #[test]
    fn a_label_speaks_before_the_bodies_that_send_to_it() {
        assert!(warnings(
            "A =>\n    emit Read { fallback = {} } to this\n    emit Read { fallback = [] } to this\n    return Ok {}\n\nRead { fallback ∈ Any } =>\n    return Ok {}\n"
        )
        .is_empty());
    }

    #[test]
    fn a_list_of_objects_may_differ_in_which_fields_they_have() {
        assert!(
            warnings("xs = [{ x = 1 }, { x = 2, z = \"a\" }]\nxs += { z = \"b\" }\n").is_empty()
        );
    }

    #[test]
    fn what_is_sent_to_one_handler_is_one_record() {
        let found = warnings(
            "Show { x } =>\n    return Ack {}\n\nemit Show { x = 1 } to this\nemit Show { x = \"far\" } to this\n",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        // Literals of one class that never meet are two values.
        assert!(warnings("a = Store { value = [1] }\nb = Store { value = \"s\" }\n").is_empty());
    }

    #[test]
    fn what_a_handler_receives_follows_its_emits() {
        let found = warnings(
            "Move { fish, by } =>\n    fish.x = fish.x + by\n    return Moved { fish = fish }\n\nemit Move { fish = { x = 1 }, by = 1 } to this\nemit Move { fish = { x = \"left\" }, by = 1 } to this\n",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        // Breaking a label is the label check's to say, as an error.
        assert!(warnings(
            "Move { fish ∈ { x ∈ Number } } =>\n    return Moved {}\n\nemit Move { fish = { x = \"left\" } } to this\n"
        )
        .is_empty());
    }

    #[test]
    fn what_a_handler_answers_is_what_get_reads() {
        let found = warnings(
            "Count {} =>\n    return Counted { value = 3 }\n\nemit Count {} to this get c\nn = c.value\nn = \"many\"\n",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("Count answers"), "{}", found[0]);
    }

    #[test]
    fn a_computed_name_makes_a_map() {
        let found = warnings(
            "counts = {}\nname = \"a\"\ncounts[name] = 1\ncounts = counts + { \"$name\" = 2 }\ncounts.b = \"x\"\n",
        );
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn any_is_never_reported() {
        assert!(warnings("reading ∈ Any = true\nreading = 5\nreading = \"on\"\n").is_empty());
    }

    #[test]
    fn a_module_answer_is_not_guessed_at() {
        assert!(warnings(
            "emit Read { key = \"a\" } to core get r\nemit Read { key = \"b\" } to core get r\nx = r.value\nx = 5\nx = \"s\"\n"
        )
        .is_empty());
    }

    #[test]
    fn separate_blocks_bind_separate_names() {
        assert!(warnings("if true\n    t = 1\nif true\n    t = \"x\"\n").is_empty());
    }
}

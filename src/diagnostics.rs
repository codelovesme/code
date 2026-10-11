use std::collections::HashMap;

use crate::ast::{
    BinOp, EmitTarget, Expr, Field, FieldKey, Label, Program, Span, Stmt, UnOp, ValueKind,
};
use crate::span::Origin;

/// Severity used by the machine-readable checker report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

/// One diagnostic produced by `code check`.
///
/// The optional context fields are deliberately structured rather than folded
/// into the message. Agents can use the stable code and fields without
/// parsing prose. `span` preserves the source range when this diagnostic came
/// from a parsed program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
    pub handler: Option<String>,
    pub particle: Option<String>,
    pub target: Option<String>,
    pub field: Option<String>,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub suggestion: Option<String>,
    /// The source range responsible for this diagnostic, when the checker was
    /// given a parsed source program.
    pub span: Option<Span>,
    /// The source origin for `span`; linked imports carry their own origin.
    pub origin: Option<Origin>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StaticType {
    Kind(ValueKind),
    Class(String),
}

impl StaticType {
    fn name(&self) -> String {
        match self {
            Self::Kind(kind) => kind.name().to_string(),
            Self::Class(class_name) => class_name.clone(),
        }
    }
}

struct CheckContext<'a> {
    origin: Option<&'a Origin>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExpectedType {
    Kind(ValueKind),
    Class(String),
}

impl ExpectedType {
    fn from_name(name: &str) -> Self {
        ValueKind::parse(name)
            .map(Self::Kind)
            .unwrap_or_else(|| Self::Class(name.to_string()))
    }

    fn accepts(&self, actual: &StaticType) -> bool {
        match (self, actual) {
            (Self::Kind(expected), StaticType::Kind(actual)) => expected == actual,
            // Every particle is an Object at runtime, while a named class is
            // more specific than Object for static checking.
            (Self::Kind(ValueKind::Object), StaticType::Class(_)) => true,
            (Self::Kind(_), StaticType::Class(_)) => false,
            (Self::Class(expected), StaticType::Class(actual)) => expected == actual,
            (Self::Class(_), StaticType::Kind(_)) => false,
        }
    }
}

/// Checks statically knowable local handler calls and gradual annotations
/// without changing runtime dispatch. An open message vocabulary remains
/// valid at runtime; this command is the explicit development-time check that
/// lets an AI catch a likely typo before running the program.
///
/// Each resolved source module has its own `to this` handler table. Collecting
/// a table before walking its bodies permits forward references while keeping
/// a module from accidentally validating a call against a different module's
/// handlers. Dynamic particle expressions are left alone because their class
/// is not knowable here.
pub fn check_handlers(program: &Program) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    check_file(
        &program.statements,
        program.origin.as_ref(),
        &mut diagnostics,
    );
    diagnostics
}

pub fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
}

fn check_file(statements: &[Stmt], origin: Option<&Origin>, diagnostics: &mut Vec<Diagnostic>) {
    let handlers: HashMap<String, Vec<Field>> = statements
        .iter()
        .filter_map(|statement| match statement {
            Stmt::HandlerDef {
                class_name, fields, ..
            } => Some((class_name.clone(), fields.clone())),
            _ => None,
        })
        .collect();
    let mut types = HashMap::new();
    check_statements(statements, &handlers, None, &mut types, origin, diagnostics);
}

fn check_statements(
    statements: &[Stmt],
    handlers: &HashMap<String, Vec<Field>>,
    current_handler: Option<&str>,
    types: &mut HashMap<String, StaticType>,
    origin: Option<&Origin>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for statement in statements {
        match statement {
            Stmt::HandlerDef {
                class_name,
                fields,
                body,
            } => {
                let mut handler_types = HashMap::new();
                for field in fields {
                    if let Some(known) = field
                        .annotation
                        .as_ref()
                        .and_then(static_type_from_annotation)
                    {
                        handler_types.insert(field.name.clone(), known);
                    }
                }
                check_statements(
                    body,
                    handlers,
                    Some(class_name),
                    &mut handler_types,
                    origin,
                    diagnostics,
                );
            }
            Stmt::Import {
                body,
                origin: imported_origin,
                ..
            } => check_file(body, Some(imported_origin), diagnostics),
            Stmt::If { body, .. } | Stmt::Loop { body, .. } => {
                let mut nested_types = types.clone();
                check_statements(
                    body,
                    handlers,
                    current_handler,
                    &mut nested_types,
                    origin,
                    diagnostics,
                );
            }
            Stmt::Assign {
                name,
                annotation,
                value,
                span,
            } => {
                if let Some(annotation) = annotation {
                    let mut context = CheckContext {
                        origin,
                        diagnostics,
                    };
                    check_value_against_annotation(
                        annotation,
                        value,
                        types,
                        current_handler,
                        &mut context,
                        Some(name),
                        *span,
                    );
                    match static_type_from_annotation(annotation) {
                        Some(known) => types.insert(name.clone(), known),
                        None => types.remove(name),
                    };
                } else if let Some(value_type) = static_type(value, types) {
                    types.insert(name.clone(), value_type);
                } else {
                    types.remove(name);
                }
            }
            Stmt::Emit {
                particle,
                target,
                span,
                ..
            } => {
                let mut context = CheckContext {
                    origin,
                    diagnostics,
                };
                check_emit(
                    particle,
                    target,
                    handlers,
                    current_handler,
                    types,
                    &mut context,
                    *span,
                );
            }
            Stmt::Return { value, span } => {
                let mut context = CheckContext {
                    origin,
                    diagnostics,
                };
                check_return(value, current_handler, types, &mut context, *span)
            }
            _ => {}
        }
    }
}

fn check_emit(
    particle: &Expr,
    target: &EmitTarget,
    handlers: &HashMap<String, Vec<Field>>,
    current_handler: Option<&str>,
    types: &HashMap<String, StaticType>,
    context: &mut CheckContext<'_>,
    span: Option<Span>,
) {
    if !matches!(target, EmitTarget::This) {
        return;
    }
    if let Some(actual_name) = static_non_particle_type(particle, types) {
        context.diagnostics.push(Diagnostic {
            code: "non-particle",
            severity: Severity::Error,
            message: format!(
                "emit requires a particle, but the statically known value is {actual_name}"
            ),
            handler: current_handler.map(str::to_owned),
            particle: None,
            target: Some("this".to_string()),
            field: None,
            expected: Some("particle".to_string()),
            actual: Some(actual_name),
            suggestion: None,
            span,
            origin: context.origin.cloned(),
        });
        return;
    }
    let Some(class_name) = static_particle_class(particle) else {
        return;
    };
    let Some(fields) = handlers.get(class_name) else {
        let suggestion = closest_name(class_name, handlers.keys().map(String::as_str));
        context.diagnostics.push(Diagnostic {
            code: "unknown-handler",
            severity: Severity::Error,
            message: match &suggestion {
                Some(name) => format!(
                    "no handler named '{class_name}' is defined for `to this` (try '{name}')"
                ),
                None => format!("no handler named '{class_name}' is defined for `to this`"),
            },
            handler: current_handler.map(str::to_owned),
            particle: Some(class_name.to_owned()),
            target: Some("this".to_string()),
            field: None,
            expected: None,
            actual: None,
            suggestion,
            span,
            origin: context.origin.cloned(),
        });
        return;
    };

    let Some(object_fields) = static_object_fields(particle) else {
        return;
    };
    let declared: HashMap<&str, &Field> = fields
        .iter()
        .map(|field| (field.field.as_str(), field))
        .collect();

    for &(field_name, value) in &object_fields {
        if field_name == "_class" {
            continue;
        }
        let Some(field) = declared.get(field_name) else {
            let suggestion = closest_name(field_name, declared.keys().copied());
            let expected = fields
                .iter()
                .map(|field| field.field.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            context.diagnostics.push(Diagnostic {
                code: "unknown-field",
                severity: Severity::Error,
                message: match &suggestion {
                    Some(name) => format!(
                        "handler '{class_name}' does not declare field '{field_name}' (try '{name}')"
                    ),
                    None => format!(
                        "handler '{class_name}' does not declare field '{field_name}'"
                    ),
                },
                handler: current_handler.map(str::to_owned),
                particle: Some(class_name.to_owned()),
                target: Some("this".to_string()),
                field: Some(field_name.to_owned()),
                expected: Some(expected),
                actual: Some(field_name.to_owned()),
                suggestion,
                span,
                origin: context.origin.cloned(),
            });
            continue;
        };
        if let Some(annotation) = &field.annotation {
            check_value_against_annotation(
                annotation,
                value,
                types,
                current_handler,
                context,
                Some(field_name),
                field.span,
            );
        }
    }

    for field in fields {
        let Some(annotation) = &field.annotation else {
            continue;
        };
        if !object_fields.iter().any(|(name, _)| *name == field.field) {
            let expected = annotation.spelling();
            context.diagnostics.push(Diagnostic {
                code: "missing-field",
                severity: Severity::Error,
                message: format!(
                    "handler '{class_name}' requires typed field '{}' of type {expected}",
                    field.field
                ),
                handler: current_handler.map(str::to_owned),
                particle: Some(class_name.to_owned()),
                target: Some("this".to_string()),
                field: Some(field.field.clone()),
                expected: Some(expected),
                actual: Some("missing".to_string()),
                suggestion: Some(format!("include field '{}'", field.field)),
                span: field.span,
                origin: context.origin.cloned(),
            });
        }
    }
}

fn check_return(
    value: &Expr,
    current_handler: Option<&str>,
    types: &HashMap<String, StaticType>,
    context: &mut CheckContext<'_>,
    span: Option<Span>,
) {
    let Some(actual_name) = static_non_particle_type(value, types) else {
        return;
    };
    context.diagnostics.push(Diagnostic {
        code: "non-particle",
        severity: Severity::Error,
        message: format!(
            "handler return requires a particle, but the statically known value is {actual_name}"
        ),
        handler: current_handler.map(str::to_owned),
        particle: None,
        target: None,
        field: None,
        expected: Some("particle".to_string()),
        actual: Some(actual_name),
        suggestion: None,
        span,
        origin: context.origin.cloned(),
    });
}

fn check_value_against_annotation(
    annotation: &Label,
    value: &Expr,
    types: &HashMap<String, StaticType>,
    current_handler: Option<&str>,
    context: &mut CheckContext<'_>,
    field: Option<&str>,
    span: Option<Span>,
) {
    let Some(mismatch) = label_mismatch(annotation, value, types) else {
        return;
    };
    // Where inside the value it went wrong, as a path from the field or
    // binding the label is on: `fish.x`, `shapes[2].op`.
    let at = match (field, mismatch.path.is_empty()) {
        (Some(field), _) => format!("field '{field}{}'", mismatch.path),
        (None, true) => "value for the annotated binding".to_string(),
        (None, false) => format!("'{}' of the annotated binding", &mismatch.path[1..]),
    };
    let expected = mismatch.expected;
    let actual_name = mismatch.actual;
    let message = if mismatch.unnamed {
        format!("{at} is not in its label {expected} — a label names every field, to the last one")
    } else {
        format!("{at} expects {expected}, but the statically known value is {actual_name}")
    };
    context.diagnostics.push(Diagnostic {
        code: "type-mismatch",
        severity: Severity::Error,
        message,
        handler: current_handler.map(str::to_owned),
        particle: None,
        target: None,
        field: field.map(str::to_owned),
        expected: Some(expected),
        actual: Some(actual_name),
        suggestion: None,
        span,
        origin: context.origin.cloned(),
    });
}

/// Where a statically known value does not fit a label, and how.
struct Mismatch {
    path: String,
    expected: String,
    actual: String,
    /// A field the label does not name at all, rather than one of the wrong
    /// kind; `expected` is then the label it is missing from.
    unnamed: bool,
}

/// Checks `value` against `label` as far as it is known here: a literal is
/// walked into — every field of an object literal, every element of a list
/// literal — anything else only by its kind. null fits every label (ticket
/// 122), and `Any` fits everything.
fn label_mismatch(
    label: &Label,
    value: &Expr,
    types: &HashMap<String, StaticType>,
) -> Option<Mismatch> {
    let actual = static_type(value, types)?;
    if matches!(actual, StaticType::Kind(ValueKind::Null)) {
        return None;
    }
    let wrong = |expected: String| {
        Some(Mismatch {
            path: String::new(),
            expected,
            actual: actual.name(),
            unnamed: false,
        })
    };
    let inside = |path: String, found: Mismatch| Mismatch {
        path: format!("{path}{}", found.path),
        ..found
    };
    match label {
        Label::Named(name) if name == "Any" => None,
        Label::Named(name) => {
            if ExpectedType::from_name(name).accepts(&actual) {
                None
            } else {
                wrong(name.clone())
            }
        }
        Label::Fields(fields) => {
            if !ExpectedType::Kind(ValueKind::Object).accepts(&actual) {
                return wrong(label.spelling());
            }
            let Expr::Object(literal) = value else {
                return None;
            };
            for (key, field_value) in literal {
                let FieldKey::Literal(name) = key else {
                    continue;
                };
                if name == "_class" {
                    continue;
                }
                match fields.iter().find(|(field, _)| field == name) {
                    Some((_, field_label)) => {
                        if let Some(found) = label_mismatch(field_label, field_value, types) {
                            return Some(inside(format!(".{name}"), found));
                        }
                    }
                    None => {
                        return Some(Mismatch {
                            path: format!(".{name}"),
                            expected: label.spelling(),
                            actual: "a field".to_string(),
                            unnamed: true,
                        })
                    }
                }
            }
            None
        }
        Label::ArrayOf(element) => {
            if !ExpectedType::Kind(ValueKind::Array).accepts(&actual) {
                return wrong(label.spelling());
            }
            let Expr::Array(items) = value else {
                return None;
            };
            items.iter().enumerate().find_map(|(i, item)| {
                label_mismatch(element, item, types).map(|found| inside(format!("[{i}]"), found))
            })
        }
        Label::ObjectOf(element) => {
            if !ExpectedType::Kind(ValueKind::Object).accepts(&actual) {
                return wrong(label.spelling());
            }
            let Expr::Object(literal) = value else {
                return None;
            };
            literal.iter().find_map(|(key, item)| {
                let name = match key {
                    FieldKey::Literal(name) => format!(".{name}"),
                    FieldKey::Computed(_) => "[…]".to_string(),
                };
                label_mismatch(element, item, types).map(|found| inside(name, found))
            })
        }
    }
}

/// What a label lets the checker assume about a value — nothing for `Any`.
fn static_type_from_annotation(annotation: &Label) -> Option<StaticType> {
    match annotation {
        Label::Named(name) if name == "Any" => None,
        Label::Named(name) => Some(match ValueKind::parse(name) {
            Some(kind) => StaticType::Kind(kind),
            None => StaticType::Class(name.clone()),
        }),
        Label::Fields(_) | Label::ObjectOf(_) => Some(StaticType::Kind(ValueKind::Object)),
        Label::ArrayOf(_) => Some(StaticType::Kind(ValueKind::Array)),
    }
}

fn static_non_particle_type(expr: &Expr, types: &HashMap<String, StaticType>) -> Option<String> {
    if matches!(expr, Expr::Unary(_, _)) {
        return match static_type(expr, types)? {
            StaticType::Class(_) => None,
            StaticType::Kind(kind) => Some(kind.name().to_string()),
        };
    }
    if matches!(
        expr,
        Expr::Ident(_)
            | Expr::Field(_, _)
            | Expr::Index(_, _)
            | Expr::Slice { .. }
            | Expr::SetAt { .. }
    ) {
        return None;
    }
    if expr_contains_dynamic_value(expr) {
        return None;
    }
    let actual = static_type(expr, types)?;
    match actual {
        StaticType::Class(_) => None,
        StaticType::Kind(ValueKind::Object) => {
            let Expr::Object(fields) = expr else {
                return None;
            };
            let definitely_not_a_particle = fields.iter().all(|(key, value)| match key {
                FieldKey::Literal(name) if name == "_class" => matches!(
                    value,
                    Expr::Number(_) | Expr::Bool(_) | Expr::Null | Expr::Array(_) | Expr::Object(_)
                ),
                FieldKey::Literal(_) => true,
                FieldKey::Computed(_) => false,
            });
            definitely_not_a_particle.then(|| "Object".to_string())
        }
        StaticType::Kind(kind) => Some(kind.name().to_string()),
    }
}

fn expr_contains_dynamic_value(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(_)
        | Expr::Field(_, _)
        | Expr::Index(_, _)
        | Expr::Slice { .. }
        | Expr::SetAt { .. } => true,
        Expr::Array(_) | Expr::Object(_) => false,
        Expr::Number(_) | Expr::Str(_) | Expr::Interpolated(_) | Expr::Bool(_) | Expr::Null => {
            false
        }
        Expr::LengthOf(value) | Expr::Unary(_, value) => expr_contains_dynamic_value(value),
        Expr::Binary(left, _, right) => {
            expr_contains_dynamic_value(left) || expr_contains_dynamic_value(right)
        }
        Expr::Is(value, _) => expr_contains_dynamic_value(value),
    }
}

fn static_type(expr: &Expr, types: &HashMap<String, StaticType>) -> Option<StaticType> {
    match expr {
        Expr::Number(_) => Some(StaticType::Kind(ValueKind::Number)),
        Expr::Str(_) | Expr::Interpolated(_) => Some(StaticType::Kind(ValueKind::String)),
        Expr::Bool(_) => Some(StaticType::Kind(ValueKind::Boolean)),
        Expr::Null => Some(StaticType::Kind(ValueKind::Null)),
        Expr::Ident(name) => types.get(name).cloned(),
        Expr::Array(_) => Some(StaticType::Kind(ValueKind::Array)),
        Expr::Object(fields) => match static_particle_class(expr) {
            Some(class_name) => Some(StaticType::Class(class_name.to_string())),
            None => {
                let _ = fields;
                Some(StaticType::Kind(ValueKind::Object))
            }
        },
        Expr::Field(_, _) | Expr::Index(_, _) | Expr::Slice { .. } => None,
        // Setting a part keeps the kind — an array stays an array — and the
        // class, unless the part set is the class itself.
        Expr::SetAt { base, path, .. } => match static_type(base, types)? {
            StaticType::Class(_)
                if path
                    .iter()
                    .any(|key| !matches!(key, Expr::Str(k) if k != "_class")) =>
            {
                Some(StaticType::Kind(ValueKind::Object))
            }
            known => Some(known),
        },
        Expr::LengthOf(_) => Some(StaticType::Kind(ValueKind::Number)),
        Expr::Binary(left, op, right) => static_binary_type(left, *op, right, types),
        Expr::Unary(op, value) => static_unary_type(*op, value, types),
        Expr::Is(_, _) => Some(StaticType::Kind(ValueKind::Boolean)),
    }
}

fn static_unary_type(
    op: UnOp,
    value: &Expr,
    types: &HashMap<String, StaticType>,
) -> Option<StaticType> {
    match (op, static_type(value, types)?) {
        (UnOp::Neg, StaticType::Kind(ValueKind::Number)) => {
            Some(StaticType::Kind(ValueKind::Number))
        }
        (UnOp::Not, StaticType::Kind(ValueKind::Boolean)) => {
            Some(StaticType::Kind(ValueKind::Boolean))
        }
        _ => None,
    }
}

fn static_binary_type(
    left: &Expr,
    op: BinOp,
    right: &Expr,
    types: &HashMap<String, StaticType>,
) -> Option<StaticType> {
    match op {
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
            Some(StaticType::Kind(ValueKind::Boolean))
        }
        BinOp::And | BinOp::Or => match (static_type(left, types), static_type(right, types)) {
            (
                Some(StaticType::Kind(ValueKind::Boolean)),
                Some(StaticType::Kind(ValueKind::Boolean)),
            ) => Some(StaticType::Kind(ValueKind::Boolean)),
            _ => None,
        },
        BinOp::Sub | BinOp::Mul | BinOp::Div => {
            match (static_type(left, types), static_type(right, types)) {
                (
                    Some(StaticType::Kind(ValueKind::Number)),
                    Some(StaticType::Kind(ValueKind::Number)),
                ) => Some(StaticType::Kind(ValueKind::Number)),
                _ => None,
            }
        }
        BinOp::Add => match (static_type(left, types), static_type(right, types)) {
            (Some(StaticType::Kind(ValueKind::String)), _)
            | (_, Some(StaticType::Kind(ValueKind::String))) => {
                Some(StaticType::Kind(ValueKind::String))
            }
            (
                Some(StaticType::Kind(ValueKind::Number)),
                Some(StaticType::Kind(ValueKind::Number)),
            ) => Some(StaticType::Kind(ValueKind::Number)),
            (Some(StaticType::Kind(ValueKind::Array)), _)
            | (_, Some(StaticType::Kind(ValueKind::Array))) => {
                Some(StaticType::Kind(ValueKind::Array))
            }
            (
                Some(StaticType::Kind(ValueKind::Object)),
                Some(StaticType::Kind(ValueKind::Object)),
            ) => Some(StaticType::Kind(ValueKind::Object)),
            _ => None,
        },
    }
}

fn static_particle_class(expr: &Expr) -> Option<&str> {
    let Expr::Object(fields) = expr else {
        return None;
    };
    fields.iter().find_map(|(key, value)| {
        if !matches!(key, FieldKey::Literal(name) if name == "_class") {
            return None;
        }
        match value {
            Expr::Str(class_name) => Some(class_name.as_str()),
            _ => None,
        }
    })
}

fn static_object_fields(expr: &Expr) -> Option<Vec<(&str, &Expr)>> {
    let Expr::Object(fields) = expr else {
        return None;
    };
    Some(
        fields
            .iter()
            .filter_map(|(key, value)| match key {
                FieldKey::Literal(name) => Some((name.as_str(), value)),
                FieldKey::Computed(_) => None,
            })
            .collect(),
    )
}

fn closest_name<'a, I>(needle: &str, candidates: I) -> Option<String>
where
    I: IntoIterator<Item = &'a str>,
{
    candidates
        .into_iter()
        .filter_map(|candidate| {
            let distance = edit_distance(needle, candidate);
            let limit = (needle.chars().count() / 3).max(2);
            (distance <= limit).then_some((distance, candidate))
        })
        .min_by(|(left_distance, left_name), (right_distance, right_name)| {
            left_distance
                .cmp(right_distance)
                .then_with(|| left_name.cmp(right_name))
        })
        .map(|(_, candidate)| candidate.to_string())
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for (left_index, left_char) in left.chars().enumerate() {
        let mut current = vec![left_index + 1];
        for (right_index, right_char) in right.iter().enumerate() {
            let cost = usize::from(left_char != *right_char);
            current.push(
                (current[right_index] + 1)
                    .min(previous[right_index + 1] + 1)
                    .min(previous[right_index] + cost),
            );
        }
        previous = current;
    }
    previous[right.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer, parser};

    fn parse(source: &str) -> Program {
        let lexed = lexer::tokenize(source).expect("tokenize source");
        parser::parse(&lexed).expect("parse source")
    }

    #[test]
    fn catches_a_static_local_handler_typo() {
        let program = parse("Known {} =>\n    return Ack {}\n\nemit Unknown {} to this\n");
        let diagnostics = check_handlers(&program);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unknown-handler");
        assert_eq!(diagnostics[0].particle.as_deref(), Some("Unknown"));
    }

    #[test]
    fn leaves_dynamic_particles_for_runtime_dispatch() {
        let program =
            parse("Known {} =>\n    return Ack {}\n\nparticle = Known {}\nemit particle to this\n");
        assert!(check_handlers(&program).is_empty());
    }

    /// Labels say what is inside, to the last field (ticket 122): a wrong
    /// leaf is found and named by its path.
    #[test]
    fn checks_nested_labels_to_the_leaf() {
        let source = "Move { fish ∈ { x ∈ Number, y ∈ Number }, by ∈ Number } =>\n    return Ack {}\n\nemit Move { fish = { x = 1, y = \"two\" }, by = 3 } to this\n";
        let diagnostics = check_handlers(&parse(source));
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "type-mismatch");
        assert_eq!(diagnostics[0].expected.as_deref(), Some("Number"));
        assert_eq!(diagnostics[0].actual.as_deref(), Some("String"));
        assert!(
            diagnostics[0].message.contains("field 'fish.y'"),
            "{}",
            diagnostics[0].message
        );
    }

    #[test]
    fn checks_list_elements_and_map_values() {
        let source = "shapes ∈ Array of { op ∈ String, x ∈ Number } = [{ op = \"dot\", x = 1 }, { op = 2, x = 3 }]\ncounts ∈ Object of Number = { a = 1, b = \"x\" }\ngrid ∈ Array of Array of Number = [[1, 2], [3, true]]\n";
        let diagnostics = check_handlers(&parse(source));
        let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(diagnostics.len(), 3, "{messages:?}");
        assert!(messages[0].contains("'shapes[1].op'"), "{}", messages[0]);
        assert!(messages[1].contains("'counts.b'"), "{}", messages[1]);
        assert!(messages[2].contains("'grid[1][1]'"), "{}", messages[2]);
    }

    #[test]
    fn null_fits_every_label_and_any_fits_everything() {
        let source = "count ∈ Number = null\nfish ∈ { x ∈ Number, y ∈ Number } = { x = null, y = 2 }\nreading ∈ Any = \"on\"\nreading = 5\nboxes ∈ Array of { w ∈ Number } = [null, { w = 1 }]\n";
        let diagnostics = check_handlers(&parse(source));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn a_field_the_label_does_not_name_is_reported() {
        let source = "point ∈ { x ∈ Number, y ∈ Number } = { x = 1, y = 2, z = 3 }\n";
        let diagnostics = check_handlers(&parse(source));
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("'point.z' is not in its label"),
            "{}",
            diagnostics[0].message
        );
    }

    #[test]
    fn labels_read_back_the_way_they_are_written() {
        let source = "Draw { shapes ∈ Array of { op ∈ String, at ∈ { x ∈ Number } }, counts ∈ Object of Number, tag ∈ Any } =>\n    return Ack {}\n";
        let program = parse(source);
        let Stmt::HandlerDef { fields, .. } = &program.statements[0] else {
            panic!("expected a handler");
        };
        let spelled: Vec<String> = fields
            .iter()
            .map(|f| f.annotation.as_ref().unwrap().spelling())
            .collect();
        assert_eq!(
            spelled,
            [
                "Array of { op ∈ String, at ∈ { x ∈ Number } }",
                "Object of Number",
                "Any"
            ]
        );
    }

    #[test]
    fn of_stays_an_ordinary_name_outside_a_label() {
        let source = "parts = { of = 1, array = 2 }\nassert parts.of = 1\nlist ∈ Array = [1]\n";
        assert!(check_handlers(&parse(source)).is_empty());
    }

    #[test]
    fn a_label_field_without_its_own_label_is_refused() {
        let lexed = lexer::tokenize("Move { fish ∈ { x, y } } =>\n    return Ack {}\n").unwrap();
        let error = parser::parse(&lexed).unwrap_err();
        assert!(
            error.msg.contains("field 'x' in a label needs one too"),
            "{}",
            error.msg
        );
    }

    #[test]
    fn checks_annotated_assignments() {
        let source = "count ∈ Number = \"three\"\n";
        let program = parse(source);
        let diagnostics = check_handlers(&program);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "type-mismatch");
        assert_eq!(diagnostics[0].expected.as_deref(), Some("Number"));
        assert_eq!(diagnostics[0].actual.as_deref(), Some("String"));
        assert_eq!(
            diagnostics[0].span,
            Some(crate::ast::Span {
                start: 0,
                end: source.trim_end().chars().count() as u32,
            })
        );
    }

    #[test]
    fn checks_handler_fields_and_suggests_typos() {
        let source =
            "Grade { score ∈ Number } =>\n    return Ack {}\n\nemit Grade { score = \"bad\", scoer = 1 } to this\n";
        let program = parse(source);
        let diagnostics = check_handlers(&program);
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].code, "type-mismatch");
        let field_start = source.find("score").unwrap() as u32;
        assert_eq!(
            diagnostics[0].span,
            Some(crate::ast::Span {
                start: field_start,
                end: field_start + "score ∈ Number".chars().count() as u32,
            })
        );
        assert_eq!(diagnostics[1].code, "unknown-field");
        assert_eq!(diagnostics[1].suggestion.as_deref(), Some("score"));
        let emit_start = source
            .find("emit")
            .map(|byte_offset| source[..byte_offset].chars().count())
            .unwrap() as u32;
        let emit_end = source.trim_end().chars().count() as u32;
        assert_eq!(
            diagnostics[1].span,
            Some(crate::ast::Span {
                start: emit_start,
                end: emit_end,
            })
        );
    }

    #[test]
    fn reports_a_missing_typed_handler_field() {
        let source = "Grade { score ∈ Number } =>\n    return Ack {}\n\nemit Grade {} to this\n";
        let program = parse(source);
        let diagnostics = check_handlers(&program);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "missing-field");
        assert_eq!(diagnostics[0].field.as_deref(), Some("score"));
        let field_start = source.find("score").unwrap() as u32;
        assert_eq!(
            diagnostics[0].span,
            Some(crate::ast::Span {
                start: field_start,
                end: field_start + "score ∈ Number".chars().count() as u32,
            })
        );
    }

    #[test]
    fn attaches_the_emit_boundary_to_unknown_handler_diagnostics() {
        let source = "emit Unknown {} to this\n";
        let program = parse(source);
        let diagnostics = check_handlers(&program);
        assert_eq!(
            diagnostics[0].span,
            Some(crate::ast::Span {
                start: 0,
                end: source.trim_end().chars().count() as u32,
            })
        );
    }

    #[test]
    fn reports_a_static_non_particle_emit_boundary() {
        let source = "emit 5 to this\n";
        let program = parse(source);
        let diagnostics = check_handlers(&program);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "non-particle");
        assert_eq!(
            diagnostics[0].span,
            Some(crate::ast::Span {
                start: 0,
                end: source.trim_end().chars().count() as u32,
            })
        );
    }

    #[test]
    fn reports_a_static_unary_non_particle_from_a_known_binding() {
        let program = parse("x = 1\nemit -x to this\n");
        let diagnostics = check_handlers(&program);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "non-particle");
        assert_eq!(diagnostics[0].actual.as_deref(), Some("Number"));
    }

    #[test]
    fn leaves_unchecked_unary_operands_dynamic() {
        let program = parse("x = other\nemit -x to this\n");
        assert!(check_handlers(&program).is_empty());
    }

    #[test]
    fn does_not_check_static_values_sent_to_non_this_targets() {
        let program = parse("emit 5 to core\n");
        assert!(check_handlers(&program).is_empty());
    }

    #[test]
    fn leaves_dynamic_particle_values_unchecked() {
        let program = parse("particle = 5\nemit particle to this\n");
        assert!(check_handlers(&program).is_empty());
    }

    #[test]
    fn leaves_dynamic_particle_classes_unchecked() {
        let program = parse("emit { _class = class_name } to this\n");
        assert!(check_handlers(&program).is_empty());
    }

    #[test]
    fn reports_a_static_non_particle_handler_return() {
        let program = parse("Known {} =>\n    return 5\n");
        let diagnostics = check_handlers(&program);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "non-particle");
        assert_eq!(diagnostics[0].handler.as_deref(), Some("Known"));
        assert_eq!(
            diagnostics[0].span,
            Some(crate::ast::Span {
                start: "Known {} =>\n    ".chars().count() as u32,
                end: "Known {} =>\n    return 5".chars().count() as u32,
            })
        );
    }
}

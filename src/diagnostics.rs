use std::collections::HashMap;

use crate::ast::{BinOp, EmitTarget, Expr, Field, FieldKey, Program, Stmt, UnOp, ValueKind};

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
/// parsing prose. Source spans will be added when the AST carries them; this
/// slice identifies the handler, particle, field, and type exactly.
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
    check_file(&program.statements, &mut diagnostics);
    diagnostics
}

pub fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
}

fn check_file(statements: &[Stmt], diagnostics: &mut Vec<Diagnostic>) {
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
    check_statements(statements, &handlers, None, &mut types, diagnostics);
}

fn check_statements(
    statements: &[Stmt],
    handlers: &HashMap<String, Vec<Field>>,
    current_handler: Option<&str>,
    types: &mut HashMap<String, StaticType>,
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
                    if let Some(annotation) = &field.annotation {
                        handler_types
                            .insert(field.name.clone(), static_type_from_annotation(annotation));
                    }
                }
                check_statements(
                    body,
                    handlers,
                    Some(class_name),
                    &mut handler_types,
                    diagnostics,
                );
            }
            Stmt::Import { body, .. } => check_file(body, diagnostics),
            Stmt::If { body, .. } | Stmt::Loop { body, .. } => {
                let mut nested_types = types.clone();
                check_statements(
                    body,
                    handlers,
                    current_handler,
                    &mut nested_types,
                    diagnostics,
                );
            }
            Stmt::Assign {
                name,
                annotation,
                value,
            } => {
                if let Some(annotation) = annotation {
                    check_value_against_annotation(
                        annotation,
                        value,
                        types,
                        current_handler,
                        diagnostics,
                        Some(name),
                    );
                    types.insert(name.clone(), static_type_from_annotation(annotation));
                } else if let Some(value_type) = static_type(value, types) {
                    types.insert(name.clone(), value_type);
                } else {
                    types.remove(name);
                }
            }
            Stmt::Emit {
                particle, target, ..
            } => check_emit(
                particle,
                target,
                handlers,
                current_handler,
                types,
                diagnostics,
            ),
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
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !matches!(target, EmitTarget::This) {
        return;
    }
    let Some(class_name) = static_particle_class(particle) else {
        return;
    };
    let Some(fields) = handlers.get(class_name) else {
        let suggestion = closest_name(class_name, handlers.keys().map(String::as_str));
        diagnostics.push(Diagnostic {
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
            diagnostics.push(Diagnostic {
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
            });
            continue;
        };
        if let Some(annotation) = &field.annotation {
            check_value_against_annotation(
                annotation,
                value,
                types,
                current_handler,
                diagnostics,
                Some(field_name),
            );
        }
    }

    for field in fields {
        let Some(annotation) = &field.annotation else {
            continue;
        };
        if !object_fields.iter().any(|(name, _)| *name == field.field) {
            let expected = annotation.clone();
            diagnostics.push(Diagnostic {
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
            });
        }
    }
}

fn check_value_against_annotation(
    annotation: &str,
    value: &Expr,
    types: &HashMap<String, StaticType>,
    current_handler: Option<&str>,
    diagnostics: &mut Vec<Diagnostic>,
    field: Option<&str>,
) {
    let Some(actual) = static_type(value, types) else {
        return;
    };
    let expected = ExpectedType::from_name(annotation);
    if expected.accepts(&actual) {
        return;
    }
    let actual_name = actual.name();
    diagnostics.push(Diagnostic {
        code: "type-mismatch",
        severity: Severity::Error,
        message: match field {
            Some(field) => format!(
                "field '{field}' expects {annotation}, but the statically known value is {actual_name}"
            ),
            None => format!(
                "value for the annotated binding expects {annotation}, but the statically known value is {actual_name}"
            ),
        },
        handler: current_handler.map(str::to_owned),
        particle: None,
        target: None,
        field: field.map(str::to_owned),
        expected: Some(annotation.to_string()),
        actual: Some(actual_name),
        suggestion: None,
    });
}

fn static_type_from_annotation(annotation: &str) -> StaticType {
    match ValueKind::parse(annotation) {
        Some(kind) => StaticType::Kind(kind),
        None => StaticType::Class(annotation.to_string()),
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
        Expr::LengthOf(_) => Some(StaticType::Kind(ValueKind::Number)),
        Expr::Binary(left, op, right) => static_binary_type(left, *op, right, types),
        Expr::Unary(op, _value) => match op {
            UnOp::Neg => Some(StaticType::Kind(ValueKind::Number)),
            UnOp::Not => Some(StaticType::Kind(ValueKind::Boolean)),
        },
        Expr::Is(_, _) => Some(StaticType::Kind(ValueKind::Boolean)),
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

    #[test]
    fn checks_annotated_assignments() {
        let program = parse("count ∈ Number = \"three\"\n");
        let diagnostics = check_handlers(&program);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "type-mismatch");
        assert_eq!(diagnostics[0].expected.as_deref(), Some("Number"));
        assert_eq!(diagnostics[0].actual.as_deref(), Some("String"));
    }

    #[test]
    fn checks_handler_fields_and_suggests_typos() {
        let program = parse(
            "Grade { score ∈ Number } =>\n    return Ack {}\n\nemit Grade { score = \"bad\", scoer = 1 } to this\n",
        );
        let diagnostics = check_handlers(&program);
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].code, "type-mismatch");
        assert_eq!(diagnostics[1].code, "unknown-field");
        assert_eq!(diagnostics[1].suggestion.as_deref(), Some("score"));
    }

    #[test]
    fn reports_a_missing_typed_handler_field() {
        let program =
            parse("Grade { score ∈ Number } =>\n    return Ack {}\n\nemit Grade {} to this\n");
        let diagnostics = check_handlers(&program);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "missing-field");
        assert_eq!(diagnostics[0].field.as_deref(), Some("score"));
    }
}

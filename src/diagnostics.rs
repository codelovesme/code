use std::collections::HashMap;

use crate::ast::{EmitTarget, Expr, FieldKey, Program, Stmt};

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
/// first slice still identifies the handler, particle, and target exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
    pub handler: Option<String>,
    pub particle: Option<String>,
    pub target: Option<String>,
}

/// Checks statically knowable local handler calls without changing runtime
/// dispatch. An open message vocabulary remains valid at runtime; this
/// command is the explicit development-time check that lets an AI catch a
/// likely typo before running the program.
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
    let handlers: HashMap<&str, ()> = statements
        .iter()
        .filter_map(|statement| match statement {
            Stmt::HandlerDef { class_name, .. } => Some((class_name.as_str(), ())),
            _ => None,
        })
        .collect();
    check_statements(statements, &handlers, None, diagnostics);
}

fn check_statements(
    statements: &[Stmt],
    handlers: &HashMap<&str, ()>,
    current_handler: Option<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for statement in statements {
        match statement {
            Stmt::HandlerDef {
                class_name, body, ..
            } => check_statements(body, handlers, Some(class_name), diagnostics),
            Stmt::Import { body, .. } => check_file(body, diagnostics),
            Stmt::If { body, .. } | Stmt::Loop { body, .. } => {
                check_statements(body, handlers, current_handler, diagnostics)
            }
            Stmt::Emit {
                particle, target, ..
            } => check_emit(particle, target, handlers, current_handler, diagnostics),
            _ => {}
        }
    }
}

fn check_emit(
    particle: &Expr,
    target: &EmitTarget,
    handlers: &HashMap<&str, ()>,
    current_handler: Option<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !matches!(target, EmitTarget::This) {
        return;
    }
    let Some(class_name) = static_particle_class(particle) else {
        return;
    };
    if handlers.contains_key(class_name) {
        return;
    }

    let origin = current_handler.unwrap_or("top level");
    diagnostics.push(Diagnostic {
        code: "unknown-handler",
        severity: Severity::Error,
        message: format!(
            "no handler named '{class_name}' is defined for `to this` (emitted from {origin})"
        ),
        handler: current_handler.map(str::to_owned),
        particle: Some(class_name.to_owned()),
        target: Some("this".to_string()),
    });
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
}

use crate::ast::{Expr, Program, Stmt};

/// The statically discoverable shape of one source handler.
///
/// This is intentionally smaller than a type system. It reports the names an
/// agent needs in order to construct the input particle, while leaving runtime
/// dispatch and the language's permissive message semantics unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "install", derive(serde::Serialize, serde::Deserialize))]
pub struct HandlerDescription {
    pub name: String,
    pub fields: Vec<FieldDescription>,
    /// A result class is reported only when every statically visible return in
    /// the body names the same literal particle class. Dynamic or conflicting
    /// returns remain unknown rather than being guessed.
    #[cfg_attr(
        feature = "install",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub result_class: Option<String>,
}

/// One field in a handler's input particle.
///
/// `wire_name` is the field the sender puts in the particle. `binding_name` is
/// the name the handler body reads after an optional `as` rename. Native module
/// contracts may omit the binding name because there is no source body whose
/// local naming can be observed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "install", derive(serde::Serialize, serde::Deserialize))]
pub struct FieldDescription {
    pub wire_name: String,
    #[cfg_attr(
        feature = "install",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub binding_name: Option<String>,
    #[cfg_attr(
        feature = "install",
        serde(rename = "type", default, skip_serializing_if = "Option::is_none")
    )]
    pub type_name: Option<String>,
}

/// The capability data shown for one module. `None` means the module did not
/// declare that capability; an empty collection means it explicitly declared
/// that it has none. That distinction prevents an agent from mistaking absent
/// metadata for a safety guarantee.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleCapabilityDescription {
    pub effects: Option<Vec<String>>,
    pub configuration: Option<ConfigurationDescription>,
    pub timeouts: Option<Vec<TimeoutDescription>>,
    pub handler_contracts: Option<Vec<HandlerDescription>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigurationDescription {
    pub handler: String,
    pub fields: Option<Vec<FieldDescription>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeoutDescription {
    pub handler: String,
    pub milliseconds: u64,
}

/// A module in the catalog. Source modules get contracts from their handler
/// declarations. Native modules get only the names and metadata explicitly
/// supplied by their manifest; missing native metadata stays empty/unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDescription {
    pub name: String,
    pub kind: ModuleKind,
    pub handlers: Vec<String>,
    pub capabilities: ModuleCapabilityDescription,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleKind {
    Source,
    Native,
}

impl ModuleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Native => "native",
        }
    }
}

/// Collects source handlers in deterministic source order.
///
/// Resolved `.code` modules appear as `Stmt::Import` bodies, so walking those
/// bodies makes the catalog cover the same source tree that the loader gives
/// to both execution backends. Native modules are deliberately absent here:
/// their handler contracts are not present in the AST and must come from
/// machine-readable module metadata rather than guesses.
pub fn describe_handlers(program: &Program) -> Vec<HandlerDescription> {
    let mut handlers = Vec::new();
    collect_handlers(&program.statements, &mut handlers);
    handlers
}

/// Describes the source modules represented by a resolved program. Each module
/// keeps its own direct handlers while `describe_handlers` remains the flat,
/// backwards-compatible source-order catalog used by existing callers.
pub fn describe_source_modules(program: &Program) -> Vec<ModuleDescription> {
    let mut modules = Vec::new();
    let name = program
        .origin
        .as_ref()
        .map(|origin| origin.file.clone())
        .unwrap_or_else(|| "<entry>".to_string());
    collect_source_module(&name, &program.statements, &mut modules);
    modules
}

/// Returns native links in deterministic source order. The loader keeps native
/// imports inside the source module body that declared them, so walking imports
/// here mirrors handler discovery without inspecting or opening native bytes.
pub fn native_links(program: &Program) -> Vec<(String, String)> {
    let mut links = Vec::new();
    collect_native_links(&program.statements, &mut links);
    links
}

fn collect_native_links(statements: &[Stmt], links: &mut Vec<(String, String)>) {
    for statement in statements {
        match statement {
            Stmt::ImportNative { alias, path, .. } => links.push((alias.clone(), path.clone())),
            Stmt::Import { body, .. } => collect_native_links(body, links),
            _ => {}
        }
    }
}

fn collect_source_module(name: &str, statements: &[Stmt], modules: &mut Vec<ModuleDescription>) {
    let handlers = direct_handlers(statements);
    modules.push(ModuleDescription {
        name: name.to_string(),
        kind: ModuleKind::Source,
        handlers: handlers
            .iter()
            .map(|handler| handler.name.clone())
            .collect(),
        capabilities: ModuleCapabilityDescription {
            effects: None,
            configuration: None,
            timeouts: None,
            handler_contracts: Some(handlers),
        },
    });
    for statement in statements {
        if let Stmt::Import { body, origin, .. } = statement {
            collect_source_module(&origin.file, body, modules);
        }
    }
}

fn direct_handlers(statements: &[Stmt]) -> Vec<HandlerDescription> {
    statements
        .iter()
        .filter_map(|statement| match statement {
            Stmt::HandlerDef {
                class_name,
                fields,
                body,
            } => Some(HandlerDescription {
                name: class_name.clone(),
                fields: fields
                    .iter()
                    .map(|field| FieldDescription {
                        wire_name: field.field.clone(),
                        binding_name: Some(field.name.clone()),
                        type_name: field.annotation.clone(),
                    })
                    .collect(),
                result_class: result_class(body),
            }),
            _ => None,
        })
        .collect()
}

fn collect_handlers(statements: &[Stmt], handlers: &mut Vec<HandlerDescription>) {
    for statement in statements {
        match statement {
            Stmt::HandlerDef {
                class_name,
                fields,
                body,
            } => handlers.push(HandlerDescription {
                name: class_name.clone(),
                fields: fields
                    .iter()
                    .map(|field| FieldDescription {
                        wire_name: field.field.clone(),
                        binding_name: Some(field.name.clone()),
                        type_name: field.annotation.clone(),
                    })
                    .collect(),
                result_class: result_class(body),
            }),
            Stmt::Import { body, .. } => collect_handlers(body, handlers),
            _ => {}
        }
    }
}

fn result_class(statements: &[Stmt]) -> Option<String> {
    let mut classes = Vec::new();
    let mut unknown = false;
    collect_return_classes(statements, &mut classes, &mut unknown);
    classes.dedup();
    (!unknown && classes.len() == 1).then(|| classes.pop().expect("one result class"))
}

fn collect_return_classes(statements: &[Stmt], classes: &mut Vec<String>, unknown: &mut bool) {
    for statement in statements {
        match statement {
            Stmt::Return { value, .. } => {
                if let Some(class) = literal_class(value) {
                    if !classes.contains(&class) {
                        classes.push(class);
                    }
                } else {
                    *unknown = true;
                }
            }
            Stmt::If { body, .. } | Stmt::Loop { body, .. } => {
                collect_return_classes(body, classes, unknown)
            }
            _ => {}
        }
    }
}

fn literal_class(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Object(fields) => {
            fields
                .iter()
                .find_map(|(key, value)| match (key.as_literal(), value) {
                    (Some("_class"), Expr::Str(name)) => Some(name.clone()),
                    _ => None,
                })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer, parser};

    #[test]
    fn describes_wire_names_and_body_aliases() {
        let source = "Greet { who ∈ String as person } =>\n    return Greeting {}\n";
        let lexed = lexer::tokenize(source).expect("tokenize source");
        let program = parser::parse(&lexed).expect("parse source");

        assert_eq!(
            describe_handlers(&program),
            vec![HandlerDescription {
                name: "Greet".to_string(),
                fields: vec![FieldDescription {
                    wire_name: "who".to_string(),
                    binding_name: Some("person".to_string()),
                    type_name: Some("String".to_string()),
                }],
                result_class: Some("Greeting".to_string()),
            }]
        );
    }

    #[test]
    fn leaves_dynamic_or_conflicting_handler_results_unknown() {
        let source = "One {} =>\n    answer = First {}\n    return answer\n\nTwo {} =>\n    if true\n        return First {}\n    return Second {}\n\nThree {} =>\n    if true\n        return First {}\n    return answer\n";
        let lexed = lexer::tokenize(source).expect("tokenize source");
        let program = parser::parse(&lexed).expect("parse source");
        let handlers = describe_handlers(&program);
        assert_eq!(handlers[0].result_class, None);
        assert_eq!(handlers[1].result_class, None);
        assert_eq!(handlers[2].result_class, None);
    }
}

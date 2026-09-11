use crate::ast::{Program, Stmt};

/// The statically discoverable shape of one source handler.
///
/// This is intentionally smaller than a type system. It reports the names an
/// agent needs in order to construct the input particle, while leaving runtime
/// dispatch and the language's permissive message semantics unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerDescription {
    pub name: String,
    pub fields: Vec<FieldDescription>,
}

/// One field in a handler's input particle.
///
/// `wire_name` is the field the sender puts in the particle. `binding_name` is
/// the name the handler body reads after an optional `as` rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDescription {
    pub wire_name: String,
    pub binding_name: String,
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

fn collect_handlers(statements: &[Stmt], handlers: &mut Vec<HandlerDescription>) {
    for statement in statements {
        match statement {
            Stmt::HandlerDef {
                class_name, fields, ..
            } => handlers.push(HandlerDescription {
                name: class_name.clone(),
                fields: fields
                    .iter()
                    .map(|field| FieldDescription {
                        wire_name: field.field.clone(),
                        binding_name: field.name.clone(),
                    })
                    .collect(),
            }),
            Stmt::Import { body, .. } => collect_handlers(body, handlers),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer, parser};

    #[test]
    fn describes_wire_names_and_body_aliases() {
        let source = "Greet { who as person } =>\n    return Greeting {}\n";
        let lexed = lexer::tokenize(source).expect("tokenize source");
        let program = parser::parse(&lexed).expect("parse source");

        assert_eq!(
            describe_handlers(&program),
            vec![HandlerDescription {
                name: "Greet".to_string(),
                fields: vec![FieldDescription {
                    wire_name: "who".to_string(),
                    binding_name: "person".to_string(),
                }],
            }]
        );
    }
}

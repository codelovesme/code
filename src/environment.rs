use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{ConstraintExpr, FieldConstraint, Statement, TypeExpr};
use crate::native_module::NativeFnPtr;
use crate::runtime::{Domain, Value};

/// A constrained variable: holds a domain of possible values and a frozen flag.
#[derive(Debug, Clone)]
pub struct ConstrainedVar {
    pub domain: Domain,
    /// Once frozen, no more constraints can be added.
    pub frozen: bool,
}

/// Scope-stack environment for Code's constraint-based memory model.
///
/// Variables are stored as `ConstrainedVar` with a domain and frozen flag.
/// When a constraint is applied, the domain is intersected (narrowed).
/// When a variable's value is needed, the domain must resolve to a singleton.
pub struct Environment {
    scopes: Vec<HashMap<String, ConstrainedVar>>,
    /// Type definitions (particle schemas): name -> field constraints.
    type_registry: Vec<HashMap<String, Vec<FieldConstraint>>>,
    /// Handler definitions: class_name -> handler body (statements).
    handler_registry: Vec<HashMap<String, Vec<Statement>>>,
    /// Native handler definitions: class_name -> native function wrapper.
    native_handler_registry: Vec<HashMap<String, NativeFnPtr>>,
}

impl Environment {
    /// Create a new environment with an empty global scope.
    /// Pre-registers the built-in `Exception` type.
    pub fn new() -> Self {
        let mut env = Environment {
            scopes: vec![HashMap::new()],
            type_registry: vec![HashMap::new()],
            handler_registry: vec![HashMap::new()],
            native_handler_registry: vec![HashMap::new()],
        };
        // Register built-in Exception type.
        env.define_type(
            "Exception".to_string(),
            vec![
                FieldConstraint {
                    name: "message".to_string(),
                    constraints: vec![ConstraintExpr::IsType(TypeExpr::Named(
                        "String".to_string(),
                    ))],
                    optional: false,
                },
                FieldConstraint {
                    name: "innerException".to_string(),
                    constraints: vec![ConstraintExpr::IsType(TypeExpr::Named(
                        "Exception".to_string(),
                    ))],
                    optional: true,
                },
            ],
        );
        env
    }

    /// Push a new (empty) scope frame onto the stack.
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.type_registry.push(HashMap::new());
        self.handler_registry.push(HashMap::new());
        self.native_handler_registry.push(HashMap::new());
    }

    /// Pop the innermost scope frame.
    pub fn pop_scope(&mut self) {
        if self.scopes.len() <= 1 {
            panic!("Cannot pop the global scope");
        }
        self.scopes.pop();
        self.type_registry.pop();
        self.handler_registry.pop();
        self.native_handler_registry.pop();
    }

    // -----------------------------------------------------------------------
    // Variable / constraint operations
    // -----------------------------------------------------------------------

    /// Define a variable with an exact value (equivalent to old assignment).
    pub fn define(&mut self, name: String, value: Rc<Value>) {
        self.scopes
            .last_mut()
            .expect("No active scope")
            .insert(
                name,
                ConstrainedVar {
                    domain: Domain::Exact(value),
                    frozen: false,
                },
            );
    }

    /// Define a variable with a specific domain.
    pub fn define_with_domain(&mut self, name: String, domain: Domain) {
        self.scopes
            .last_mut()
            .expect("No active scope")
            .insert(name, ConstrainedVar { domain, frozen: false });
    }

    /// Apply a constraint to an existing variable, narrowing its domain.
    /// Creates the variable if it doesn't exist.
    pub fn apply_constraint(&mut self, name: &str, new_domain: Domain) -> Result<(), String> {
        // Search for existing variable
        for scope in self.scopes.iter_mut().rev() {
            if let Some(var) = scope.get_mut(name) {
                if var.frozen {
                    return Err(format!(
                        "Cannot add constraints to frozen variable '{}'",
                        name
                    ));
                }
                let old_domain = std::mem::replace(&mut var.domain, Domain::Any);
                let new = old_domain.intersect(new_domain);
                if new.is_empty_domain() {
                    return Err(format!(
                        "Contradictory constraints for '{}': domain is empty",
                        name
                    ));
                }
                var.domain = new;
                return Ok(());
            }
        }
        // Variable doesn't exist; create it in current scope.
        if new_domain.is_empty_domain() {
            return Err(format!(
                "Contradictory constraint for '{}': domain is empty",
                name
            ));
        }
        self.scopes
            .last_mut()
            .expect("No active scope")
            .insert(
                name.to_string(),
                ConstrainedVar {
                    domain: new_domain,
                    frozen: false,
                },
            );
        Ok(())
    }

    /// Reassign an existing variable (update its domain to an exact value).
    /// Searches from innermost scope outward.
    pub fn assign(&mut self, name: &str, value: Rc<Value>) -> Result<(), String> {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(
                    name.to_string(),
                    ConstrainedVar {
                        domain: Domain::Exact(value),
                        frozen: false,
                    },
                );
                return Ok(());
            }
        }
        Err(format!("Undefined variable '{}'", name))
    }

    /// Look up a variable's resolved value.
    /// If the domain is a singleton, returns the value.
    /// Otherwise returns None.
    pub fn get(&self, name: &str) -> Option<Rc<Value>> {
        for scope in self.scopes.iter().rev() {
            if let Some(var) = scope.get(name) {
                return var.domain.is_singleton();
            }
        }
        None
    }

    /// Look up a variable's domain.
    pub fn get_domain(&self, name: &str) -> Option<&Domain> {
        for scope in self.scopes.iter().rev() {
            if let Some(var) = scope.get(name) {
                return Some(&var.domain);
            }
        }
        None
    }

    /// Check whether a variable exists in any scope.
    pub fn exists_in_any_scope(&self, name: &str) -> bool {
        self.scopes.iter().any(|scope| scope.contains_key(name))
    }

    /// Check whether a variable exists in the current (innermost) scope only.
    pub fn current_scope_has(&self, name: &str) -> bool {
        self.scopes
            .last()
            .map(|scope| scope.contains_key(name))
            .unwrap_or(false)
    }

    /// Freeze a variable (no more constraints allowed).
    pub fn freeze(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(var) = scope.get_mut(name) {
                var.frozen = true;
                return;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Type registry
    // -----------------------------------------------------------------------

    /// Register a type definition in the current scope.
    pub fn define_type(&mut self, name: String, fields: Vec<FieldConstraint>) {
        self.type_registry
            .last_mut()
            .expect("No active scope")
            .insert(name, fields);
    }

    /// Look up a type definition, searching from innermost scope outward.
    pub fn get_type(&self, name: &str) -> Option<&Vec<FieldConstraint>> {
        for scope in self.type_registry.iter().rev() {
            if let Some(fields) = scope.get(name) {
                return Some(fields);
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // Handler registry
    // -----------------------------------------------------------------------

    /// Register a handler definition in the current scope.
    pub fn define_handler(
        &mut self,
        class_name: String,
        body: Vec<Statement>,
    ) -> Result<(), String> {
        let scope = self.handler_registry.last_mut().expect("No active scope");
        if scope.contains_key(&class_name) {
            return Err(format!(
                "Duplicate handler for '{}': only one handler per particle class per scope",
                class_name
            ));
        }
        scope.insert(class_name, body);
        Ok(())
    }

    /// Look up a handler by class name, searching from innermost scope outward.
    pub fn get_handler(&self, class_name: &str) -> Option<&Vec<Statement>> {
        for scope in self.handler_registry.iter().rev() {
            if let Some(body) = scope.get(class_name) {
                return Some(body);
            }
        }
        None
    }

    /// Look up all handlers outside the current scope.
    pub fn get_handlers_outside_current_scope(&self, class_name: &str) -> Vec<Vec<Statement>> {
        self.handler_registry
            .iter()
            .rev()
            .skip(1)
            .filter_map(|scope| scope.get(class_name).cloned())
            .collect()
    }

    // -----------------------------------------------------------------------
    // Native handler registry
    // -----------------------------------------------------------------------

    pub fn define_native_handler(
        &mut self,
        class_name: String,
        func: NativeFnPtr,
    ) -> Result<(), String> {
        let scope = self
            .native_handler_registry
            .last_mut()
            .expect("No active scope");
        if scope.contains_key(&class_name) {
            return Err(format!(
                "Duplicate native handler for '{}': only one handler per particle class per scope",
                class_name
            ));
        }
        scope.insert(class_name, func);
        Ok(())
    }

    /// Replace a native handler, even if one is already registered in the current scope.
    pub fn replace_native_handler(
        &mut self,
        class_name: String,
        func: NativeFnPtr,
    ) {
        let scope = self
            .native_handler_registry
            .last_mut()
            .expect("No active scope");
        scope.insert(class_name, func);
    }

    pub fn get_native_handler(&self, class_name: &str) -> Option<&NativeFnPtr> {
        for scope in self.native_handler_registry.iter().rev() {
            if let Some(f) = scope.get(class_name) {
                return Some(f);
            }
        }
        None
    }

    pub fn get_native_handlers_outside_current_scope(
        &self,
        class_name: &str,
    ) -> Vec<NativeFnPtr> {
        self.native_handler_registry
            .iter()
            .rev()
            .skip(1)
            .filter_map(|scope| scope.get(class_name).cloned())
            .collect()
    }

    /// Save the current scope stack, replacing it with a fresh isolated scope.
    /// Type definitions are cloned into the fresh scope.
    pub fn save_and_isolate_scopes(
        &mut self,
    ) -> (
        Vec<HashMap<String, ConstrainedVar>>,
        Vec<HashMap<String, Vec<FieldConstraint>>>,
        Vec<HashMap<String, Vec<Statement>>>,
        Vec<HashMap<String, NativeFnPtr>>,
    ) {
        // Collect all types from all scopes into one map.
        let mut merged_types = HashMap::new();
        for scope in &self.type_registry {
            for (k, v) in scope {
                merged_types.insert(k.clone(), v.clone());
            }
        }

        let saved_scopes = std::mem::replace(&mut self.scopes, vec![HashMap::new()]);
        let saved_types = std::mem::replace(&mut self.type_registry, vec![merged_types]);
        let saved_handlers = std::mem::replace(&mut self.handler_registry, vec![HashMap::new()]);
        let saved_native_handlers =
            std::mem::replace(&mut self.native_handler_registry, vec![HashMap::new()]);

        (saved_scopes, saved_types, saved_handlers, saved_native_handlers)
    }

    /// Restore a previously saved scope stack.
    pub fn restore_scopes(
        &mut self,
        saved: (
            Vec<HashMap<String, ConstrainedVar>>,
            Vec<HashMap<String, Vec<FieldConstraint>>>,
            Vec<HashMap<String, Vec<Statement>>>,
            Vec<HashMap<String, NativeFnPtr>>,
        ),
    ) {
        self.scopes = saved.0;
        self.type_registry = saved.1;
        self.handler_registry = saved.2;
        self.native_handler_registry = saved.3;
    }
}

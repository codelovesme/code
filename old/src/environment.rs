use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::ast::{Spanned, Statement};
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
    /// Handler definitions: class_name -> handler body (statements).
    handler_registry: Vec<HashMap<String, Vec<Spanned<Statement>>>>,
    /// Native handler definitions: class_name -> native function wrapper.
    native_handler_registry: Vec<HashMap<String, NativeFnPtr>>,
    /// Top-level names bound by the interpreter's own bootstrap (`Particle`,
    /// `Exception` — see `Interpreter::new()`), excluded from `bindings()`
    /// so a host UI's result panel shows only what the user's own program
    /// bound, not the built-ins every program silently starts with.
    builtin_names: HashSet<String>,
}

impl Environment {
    /// Create a new environment with an empty global scope. The built-in
    /// `Particle`/`Exception` schemas are bound separately, by the
    /// interpreter executing a bootstrap source string — see
    /// `Interpreter::new()` — rather than pre-registered here, now that
    /// particle types are plain Schema variables (no more `type` keyword).
    pub fn new() -> Self {
        Environment {
            scopes: vec![HashMap::new()],
            handler_registry: vec![HashMap::new()],
            native_handler_registry: vec![HashMap::new()],
            builtin_names: HashSet::new(),
        }
    }

    /// Mark a top-level name as interpreter-bootstrapped rather than
    /// user-defined — see `Interpreter::new()`. `bindings()` excludes
    /// these.
    pub fn mark_builtin(&mut self, name: String) {
        self.builtin_names.insert(name);
    }

    /// Push a new (empty) scope frame onto the stack.
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.handler_registry.push(HashMap::new());
        self.native_handler_registry.push(HashMap::new());
    }

    /// Pop the innermost scope frame.
    pub fn pop_scope(&mut self) {
        if self.scopes.len() <= 1 {
            panic!("Cannot pop the global scope");
        }
        self.scopes.pop();
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

    /// Every top-level (global scope) binding as `(name, resolved_value)` —
    /// `None` if the variable's constraint domain wasn't narrowed to a single
    /// value (e.g. `a in Z, a > 5` with no exact value). Read-only, pure
    /// observation of the environment after a program has run — for host UIs
    /// (e.g. a playground, T19) to render a program's result; adds no
    /// language surface.
    pub fn bindings(&self) -> Vec<(String, Option<Rc<Value>>)> {
        self.scopes[0]
            .iter()
            .filter(|(name, _)| !self.builtin_names.contains(*name))
            .map(|(name, var)| (name.clone(), var.domain.is_singleton()))
            .collect()
    }

    /// Like [`bindings`](Self::bindings), but also returns a human-readable
    /// description of each unresolved binding's domain (e.g. `"3 < _ < 10"` or
    /// `"possible values: {0, 1}"`). The description is only meaningful when
    /// the resolved value is `None`; for resolved bindings it still returns the
    /// domain's description (always the value itself) but callers typically
    /// show the value instead. Lets a host UI show *what a variable could
    /// still be* rather than a bare "unresolved".
    pub fn bindings_detailed(&self) -> Vec<(String, Option<Rc<Value>>, String)> {
        self.scopes[0]
            .iter()
            .filter(|(name, _)| !self.builtin_names.contains(*name))
            .map(|(name, var)| {
                (
                    name.clone(),
                    var.domain.is_singleton(),
                    var.domain.describe(),
                )
            })
            .collect()
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
    // Handler registry
    // -----------------------------------------------------------------------

    /// Register a handler definition in the current scope.
    pub fn define_handler(
        &mut self,
        class_name: String,
        body: Vec<Spanned<Statement>>,
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
    pub fn get_handler(&self, class_name: &str) -> Option<&Vec<Spanned<Statement>>> {
        for scope in self.handler_registry.iter().rev() {
            if let Some(body) = scope.get(class_name) {
                return Some(body);
            }
        }
        None
    }

    /// Look up all handlers outside the current scope.
    pub fn get_handlers_outside_current_scope(&self, class_name: &str) -> Vec<Vec<Spanned<Statement>>> {
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

}

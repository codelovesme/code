use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use crate::ast::{
    BinaryOp, ConstraintExpr, Expression, HandlerTarget, ObjectField, Program, Spanned, Statement,
    TypeExpr, UnaryOp,
};
use crate::environment::Environment;
use crate::native_module::EmitQueue;
use crate::runtime::{values_equal, Domain, Value};

/// Dispatch a particle to a compiled-in core handler (`emit X to core get r`).
///
/// Core handlers are how Code exposes built-in behavior — the language has no
/// function-call concept, so `timestamp`/`length` and any future built-in are
/// handlers like any other, just resolved from a fixed compiled-in set instead
/// of a linked module.
fn dispatch_core_handler(class_name: &str, particle: &Value) -> Result<Rc<Value>, String> {
    let field = |name: &str| -> Option<Rc<Value>> {
        match particle {
            Value::Object(fields) => fields.get(name).cloned(),
            _ => None,
        }
    };

    match class_name {
        "Timestamp" => {
            use std::time::{SystemTime, UNIX_EPOCH};
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as f64;
            let mut result = HashMap::new();
            result.insert("_class".to_string(), Value::string("TimestampResult"));
            result.insert("value".to_string(), Value::number(ts));
            Ok(Value::object(result))
        }
        "Length" => {
            let value = field("value").ok_or_else(|| {
                "Length { value = ... } requires a 'value' field".to_string()
            })?;
            let count = match value.as_ref() {
                Value::Array(arr) => arr.len(),
                Value::String(s) => s.len(),
                other => {
                    return Err(format!(
                        "Length requires an array or string 'value', found {}",
                        other.type_name()
                    ))
                }
            };
            let mut result = HashMap::new();
            result.insert("_class".to_string(), Value::string("LengthResult"));
            result.insert("value".to_string(), Value::number(count as f64));
            Ok(Value::object(result))
        }
        other => Err(format!("Unknown core handler: '{}'", other)),
    }
}

/// Tree-walking interpreter for Code.
/// Executes a parsed AST using constraint-based variable semantics.
pub struct Interpreter {
    env: Environment,
    handler_return_value: Option<Rc<Value>>,
    in_handler_depth: usize,
    break_signal: bool,
    in_loop_depth: usize,
    emit_queues: Vec<EmitQueue>,
    keep_alive: bool,
    yield_stack: Vec<Vec<Rc<Value>>>,
    /// Span of the statement currently executing, for located runtime errors.
    current_span: Option<crate::ast::Span>,
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            env: Environment::new(),
            handler_return_value: None,
            in_handler_depth: 0,
            break_signal: false,
            in_loop_depth: 0,
            emit_queues: Vec::new(),
            keep_alive: false,
            yield_stack: Vec::new(),
            current_span: None,
        }
    }

    /// Execute an entire program.
    pub fn execute(&mut self, program: Program) -> Result<(), String> {
        for stmt in program.statements {
            self.current_span = Some(stmt.span.clone());
            self.exec_statement(stmt.node)?;
            if self.handler_return_value.is_some() {
                break;
            }
            self.drain_native_emissions()?;
        }

        // If an unhandled Exception reached root level, report it.
        if let Some(val) = self.handler_return_value.take() {
            if let Value::Object(fields) = val.as_ref() {
                let is_exception = fields
                    .get("_class")
                    .and_then(|v| {
                        if let Value::String(s) = v.as_ref() {
                            Some(s.as_str())
                        } else {
                            None
                        }
                    })
                    .map_or(false, |cls| cls == "Exception");
                if is_exception {
                    let msg = fields
                        .get("message")
                        .and_then(|v| {
                            if let Value::String(s) = v.as_ref() {
                                Some(s.clone())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| "Exception".to_string());
                    return Err(msg);
                }
            }
        }

        while self.keep_alive {
            self.drain_native_emissions()?;
            std::thread::sleep(Duration::from_millis(10));
        }

        Ok(())
    }

    /// Dispatch a particle value to the interpreter's handler registry.
    ///
    /// This is used by the cells organelle to forward HTTP requests into
    /// a loaded cell.  The particle must be an object with a `_class` field.
    /// Returns the handler's result value (typically a Respond particle).
    pub fn dispatch_particle(&mut self, particle: Rc<Value>) -> Result<Rc<Value>, String> {
        self.drain_native_emissions()?;
        let result = self.exec_handler_invoke(&particle, &HandlerTarget::This)?;
        self.drain_native_emissions()?;
        Ok(result)
    }

    /// Register a native handler function under the given class name.
    ///
    /// This allows external code (e.g. the cells organelle) to inject
    /// virtual handlers such as `server.Respond` into the interpreter's
    /// handler registry.
    pub fn register_native_handler(
        &mut self,
        class_name: String,
        func: crate::native_module::NativeFnPtr,
    ) -> Result<(), String> {
        self.env.define_native_handler(class_name, func)
    }

    /// Replace a native handler, even if one is already registered.
    pub fn replace_native_handler(
        &mut self,
        class_name: String,
        func: crate::native_module::NativeFnPtr,
    ) {
        self.env.replace_native_handler(class_name, func);
    }

    /// Override the keep-alive flag.
    ///
    /// When set to `false` before calling `execute()`, the interpreter will
    /// return after running all top-level statements instead of entering the
    /// keep-alive polling loop.  This is used by the cells organelle so each
    /// cell thread can run its own event loop.
    pub fn set_keep_alive(&mut self, keep_alive: bool) {
        self.keep_alive = keep_alive;
    }

    /// Drain native emission queues and dispatch any pending particles.
    ///
    /// This is public so external event loops (e.g. cell threads) can poll
    /// for native module emissions.
    pub fn drain_emissions(&mut self) -> Result<(), String> {
        self.drain_native_emissions()
    }

    /// Source span of the statement that was executing when the most recent
    /// error occurred. Used to render located runtime diagnostics.
    pub fn error_span(&self) -> Option<crate::ast::Span> {
        self.current_span.clone()
    }

    /// Every top-level binding after `execute()` has run: `(name,
    /// resolved_value)`, `None` if not narrowed to a single value. Read-only
    /// observation — this is a constraint language with no core I/O, so a
    /// program's visible result is its final bindings. For host UIs (e.g. a
    /// playground, T19) to render what a program produced.
    pub fn bindings(&self) -> Vec<(String, Option<Rc<crate::runtime::Value>>)> {
        self.env.bindings()
    }

    /// Execute a single statement.
    fn exec_statement(&mut self, stmt: Statement) -> Result<(), String> {
        match stmt {
            Statement::Link { module_ref, .. } => {
                return Err(format!(
                    "Unresolved module link '{}': links must be resolved before execution",
                    module_ref
                ));
            }
            Statement::Constraint {
                variable,
                constraint,
                private: _,
            } => {
                self.exec_constraint(&variable, constraint)?;
            }
            Statement::TypeDeclaration { name, fields } => {
                self.env.define_type(name, fields);
            }
            Statement::HandlerDefinition {
                class_name,
                inline_type,
                body,
            } => {
                if let Some(fields) = inline_type {
                    self.env.define_type(class_name.clone(), fields);
                }
                self.env.define_handler(class_name, body)?;
            }
            Statement::HandlerInvoke { particle, target } => {
                let val = self.eval_expr(particle)?;
                self.exec_handler_invoke(&val, &target)?;
            }
            Statement::HandlerInvokeAssign {
                particle,
                target,
                result_name,
            } => {
                let val = self.eval_expr(particle)?;
                let result = self.exec_handler_invoke(&val, &target)?;
                // Handler result is an implicit exact constraint
                if self.env.get(&result_name).is_some() {
                    self.env.assign(&result_name, result)?;
                } else {
                    self.env.define(result_name, result);
                }
            }
            Statement::HandlerReturn { value } => {
                if self.in_handler_depth == 0 {
                    return Err(
                        "Return statement 'return' used outside of a handler"
                            .to_string(),
                    );
                }
                let val = self.eval_expr(value)?;
                if self.in_handler_depth > 0 {
                    match val.as_ref() {
                        Value::Object(fields) if fields.contains_key("_class") => {}
                        _ => {
                            return Err(
                                "Handler return must be a Particle (object with _class), got a non-particle value"
                                    .to_string(),
                            )
                        }
                    }
                }
                self.handler_return_value = Some(val);
            }
            Statement::Import {
                alias,
                body,
                public_names,
                public_types,
                public_handlers,
            } => {
                self.env.push_scope();
                for stmt in body {
                    { self.current_span = Some(stmt.span.clone()); self.exec_statement(stmt.node)?; }
                }

                match alias {
                    Some(alias_name) => {
                        let mut map = HashMap::new();
                        for name in &public_names {
                            let val = self.env.get(name).ok_or_else(|| {
                                format!(
                                    "Module declares public name '{}' but it was never defined",
                                    name
                                )
                            })?;
                            map.insert(name.clone(), val);
                        }
                        self.env.pop_scope();
                        let module_val = Value::object(map);
                        self.env.define(alias_name.clone(), module_val);
                        for t in &public_types {
                            self.env.define_type(
                                format!("{}.{}", alias_name, t.name),
                                t.fields.clone(),
                            );
                        }
                        for h in &public_handlers {
                            self.env.define_handler(
                                format!("{}.{}", alias_name, h.class_name),
                                h.body.clone(),
                            )?;
                        }
                    }
                    None => {
                        let mut vals = Vec::new();
                        for name in &public_names {
                            let val = self.env.get(name).ok_or_else(|| {
                                format!(
                                    "Module declares public name '{}' but it was never defined",
                                    name
                                )
                            })?;
                            vals.push((name.clone(), val));
                        }
                        self.env.pop_scope();
                        for (name, val) in vals {
                            if self.env.get(&name).is_some() {
                                return Err(format!(
                                    "Name conflict: linked module defines '{}' which already exists in the current scope",
                                    name
                                ));
                            }
                            self.env.define(name, val);
                        }
                        for t in &public_types {
                            self.env.define_type(t.name.clone(), t.fields.clone());
                        }
                        for h in &public_handlers {
                            self.env
                                .define_handler(h.class_name.clone(), h.body.clone())?;
                        }
                    }
                }
            }
            Statement::NativeImport {
                alias,
                native_path: _,
                is_wasm: _,
                vars,
                handlers,
                types,
                emissions: _,
                emit_queue,
            } => {
                self.emit_queues.push(emit_queue);

                match alias {
                    Some(alias_name) => {
                        let mut map = HashMap::new();
                        for (name, val) in &vars {
                            map.insert(name.clone(), Rc::clone(val));
                        }
                        let module_val = Value::object(map);
                        self.env.define(alias_name.clone(), module_val);
                        for t in &types {
                            self.env.define_type(
                                format!("{}.{}", alias_name, t.name),
                                t.fields.clone(),
                            );
                        }
                        for h in &handlers {
                            self.env.define_native_handler(
                                format!("{}.{}", alias_name, h.class_name),
                                h.func.clone(),
                            )?;
                        }
                    }
                    None => {
                        for (name, val) in &vars {
                            if self.env.get(name).is_some() {
                                return Err(format!(
                                    "Name conflict: native module defines '{}' which already exists in the current scope",
                                    name
                                ));
                            }
                            self.env.define(name.clone(), Rc::clone(val));
                        }
                        for t in &types {
                            self.env.define_type(t.name.clone(), t.fields.clone());
                        }
                        for h in &handlers {
                            self.env
                                .define_native_handler(h.class_name.clone(), h.func.clone())?;
                        }
                    }
                }
            }
            Statement::Assert(expr) => {
                let val = self.eval_expr(expr)?;
                match val.as_ref() {
                    Value::Boolean(true) => {}
                    Value::Boolean(false) => {
                        let mut fields = HashMap::new();
                        fields.insert("_class".to_string(), Value::string("Exception"));
                        fields
                            .insert("message".to_string(), Value::string("Assertion failed"));
                        fields.insert("innerException".to_string(), Value::null());
                        self.handler_return_value = Some(Value::object(fields));
                    }
                    Value::Object(fields) => {
                        let is_exception = fields
                            .get("_class")
                            .and_then(|v| {
                                if let Value::String(s) = v.as_ref() {
                                    Some(s.as_str())
                                } else {
                                    None
                                }
                            })
                            .map_or(false, |cls| cls == "Exception");
                        if is_exception {
                            self.handler_return_value = Some(val);
                        }
                    }
                    _ => {}
                }
            }
            Statement::Block(stmts) => {
                self.env.push_scope();
                let result = self.exec_block_stmts(stmts);
                self.env.pop_scope();
                result?;
            }
            Statement::If { condition, body } => {
                let cond_val = self.eval_expr(condition)?;
                match cond_val.as_ref() {
                    Value::Boolean(true) => {
                        self.env.push_scope();
                        for stmt in body {
                            { self.current_span = Some(stmt.span.clone()); self.exec_statement(stmt.node)?; }
                            if self.handler_return_value.is_some() || self.break_signal {
                                break;
                            }
                        }
                        self.env.pop_scope();
                    }
                    Value::Boolean(false) => {}
                    _ => {
                        return Err(format!(
                            "If condition must be a Boolean, found {}",
                            cond_val.type_name()
                        ))
                    }
                }
            }
            Statement::LoopOver {
                variable,
                index,
                iterable,
                result,
                body,
            } => {
                let iter_val = self.eval_expr(iterable)?;
                match iter_val.as_ref() {
                    Value::Array(elements) => {
                        if result.is_some() {
                            self.yield_stack.push(Vec::new());
                        }
                        self.in_loop_depth += 1;
                        for (i, element) in elements.clone().into_iter().enumerate() {
                            self.env.push_scope();
                            self.env.define(variable.clone(), element);
                            if let Some(ref idx_name) = index {
                                self.env.define(idx_name.clone(), Value::number(i as f64));
                            }
                            for stmt in body.clone() {
                                { self.current_span = Some(stmt.span.clone()); self.exec_statement(stmt.node)?; }
                                if self.handler_return_value.is_some() || self.break_signal {
                                    break;
                                }
                            }
                            self.env.pop_scope();
                            if self.break_signal {
                                self.break_signal = false;
                                break;
                            }
                            if self.handler_return_value.is_some() {
                                break;
                            }
                        }
                        self.in_loop_depth -= 1;
                        if let Some(ref name) = result {
                            let collected = self.yield_stack.pop().unwrap_or_default();
                            self.env.define(name.clone(), Value::array(collected));
                        }
                    }
                    _ => {
                        return Err(format!(
                            "Loop requires an Array value to iterate over, found {}",
                            iter_val.type_name()
                        ))
                    }
                }
            }
            Statement::LoopInfinite { result, body } => {
                if result.is_some() {
                    self.yield_stack.push(Vec::new());
                }
                self.in_loop_depth += 1;
                loop {
                    self.env.push_scope();
                    for stmt in body.clone() {
                        { self.current_span = Some(stmt.span.clone()); self.exec_statement(stmt.node)?; }
                        self.drain_native_emissions()?;
                        if self.handler_return_value.is_some() || self.break_signal {
                            break;
                        }
                    }
                    self.env.pop_scope();
                    if self.break_signal {
                        self.break_signal = false;
                        break;
                    }
                    if self.handler_return_value.is_some() {
                        break;
                    }
                }
                self.in_loop_depth -= 1;
                if let Some(ref name) = result {
                    let collected = self.yield_stack.pop().unwrap_or_default();
                    self.env.define(name.clone(), Value::array(collected));
                }
            }
            Statement::Break => {
                if self.in_loop_depth == 0 {
                    return Err("Break statement used outside of a loop".to_string());
                }
                self.break_signal = true;
            }
            Statement::Yield(expr) => {
                let val = self.eval_expr(expr)?;
                if let Some(collector) = self.yield_stack.last_mut() {
                    collector.push(val);
                } else {
                    return Err("Yield requires a loop with 'get'".to_string());
                }
            }
        }
        Ok(())
    }

    /// Execute a constraint: apply to variable's domain.
    fn exec_constraint(
        &mut self,
        variable: &str,
        constraint: ConstraintExpr,
    ) -> Result<(), String> {
        match constraint {
            ConstraintExpr::Equals(expr) => {
                let val = self.eval_expr(expr)?;
                if self.env.get(variable).is_some() {
                    // The variable already has an exact value. Reassigning it is an
                    // imperative update, only permitted inside handler bodies; at global
                    // scope variables are single-assignment. Mirrors codegen's
                    // compile_assignment rule (reassignment allowed iff in_handler_depth > 0).
                    if self.in_handler_depth == 0 {
                        return Err(format!(
                            "Reassignment is not allowed: '{}' is single-assignment",
                            variable
                        ));
                    }
                    self.env.assign(variable, val)?;
                } else if self.env.exists_in_any_scope(variable) {
                    // Variable has domain constraints but no exact value — pin it.
                    // Goes through apply_constraint (not assign) so the pinned value
                    // is intersected against any prior narrowing (b > 3; b < 10; b = 15
                    // must be a contradiction, not a silent override).
                    self.env
                        .apply_constraint(variable, Domain::Exact(val))?;
                } else {
                    self.env.define(variable.to_string(), val);
                }
            }
            ConstraintExpr::LessThan(expr) => {
                let val = self.eval_expr(expr)?;
                match val.as_ref() {
                    Value::Number(n) => {
                        let domain = Domain::RealRange {
                            min: None,
                            max: Some(*n),
                            min_inclusive: false,
                            max_inclusive: false,
                        };
                        self.env.apply_constraint(variable, domain)?;
                    }
                    _ => return Err(format!("Cannot use '<' constraint with {}", val.type_name())),
                }
            }
            ConstraintExpr::GreaterThan(expr) => {
                let val = self.eval_expr(expr)?;
                match val.as_ref() {
                    Value::Number(n) => {
                        let domain = Domain::RealRange {
                            min: Some(*n),
                            max: None,
                            min_inclusive: false,
                            max_inclusive: false,
                        };
                        self.env.apply_constraint(variable, domain)?;
                    }
                    _ => return Err(format!("Cannot use '>' constraint with {}", val.type_name())),
                }
            }
            ConstraintExpr::LessEqual(expr) => {
                let val = self.eval_expr(expr)?;
                match val.as_ref() {
                    Value::Number(n) => {
                        let domain = Domain::RealRange {
                            min: None,
                            max: Some(*n),
                            min_inclusive: false,
                            max_inclusive: true,
                        };
                        self.env.apply_constraint(variable, domain)?;
                    }
                    _ => {
                        return Err(format!(
                            "Cannot use '≤' constraint with {}",
                            val.type_name()
                        ))
                    }
                }
            }
            ConstraintExpr::GreaterEqual(expr) => {
                let val = self.eval_expr(expr)?;
                match val.as_ref() {
                    Value::Number(n) => {
                        let domain = Domain::RealRange {
                            min: Some(*n),
                            max: None,
                            min_inclusive: true,
                            max_inclusive: false,
                        };
                        self.env.apply_constraint(variable, domain)?;
                    }
                    _ => {
                        return Err(format!(
                            "Cannot use '≥' constraint with {}",
                            val.type_name()
                        ))
                    }
                }
            }
            ConstraintExpr::NotEquals(expr) => {
                // Store as domain information (not yet used for resolution)
                let _val = self.eval_expr(expr)?;
                // For now, != constraints are tracked but don't narrow to a specific domain
                // They will be validated at resolution time
            }
            ConstraintExpr::MemberOf(expr) => {
                let val = self.eval_expr(expr)?;
                match val.as_ref() {
                    Value::Array(elements) => {
                        let domain = Domain::ValueSet(elements.clone());
                        self.env.apply_constraint(variable, domain)?;
                    }
                    _ => {
                        return Err(format!(
                            "Constraint 'in' requires an Array, got {}",
                            val.type_name()
                        ))
                    }
                }
            }
            ConstraintExpr::Domain(kind) => {
                let type_name = match kind {
                    crate::ast::DomainKind::Integer | crate::ast::DomainKind::Natural => "Number",
                    crate::ast::DomainKind::Real => "Number",
                };
                let domain =
                    Domain::TypeDomain(TypeExpr::Named(type_name.to_string()));
                self.env.apply_constraint(variable, domain)?;
            }
            ConstraintExpr::IsType(type_expr) => {
                let domain = Domain::TypeDomain(type_expr);
                self.env.apply_constraint(variable, domain)?;
            }
        }
        Ok(())
    }

    /// Execute statements inside a block.
    fn exec_block_stmts(&mut self, stmts: Vec<Spanned<Statement>>) -> Result<(), String> {
        for stmt in stmts {
            { self.current_span = Some(stmt.span.clone()); self.exec_statement(stmt.node)?; }
            if self.handler_return_value.is_some() {
                break;
            }
            if self.break_signal {
                break;
            }
        }
        Ok(())
    }

    /// Check that a value matches the expected type expression.
    #[allow(dead_code)]
    fn check_type_expr(
        &self,
        val: &Value,
        expected: &TypeExpr,
        var_name: &str,
    ) -> Result<(), String> {
        if !self.value_matches_type_expr(val, expected, &mut Vec::new()) {
            return Err(format!(
                "Type mismatch for '{}': expected {}, got {}",
                var_name,
                expected,
                val.type_name()
            ));
        }
        Ok(())
    }

    /// Recursively check if a value matches a type expression.
    fn value_matches_type_expr(
        &self,
        val: &Value,
        type_expr: &TypeExpr,
        seen: &mut Vec<String>,
    ) -> bool {
        match type_expr {
            TypeExpr::Named(name) => {
                match name.as_str() {
                    "Number" => matches!(val, Value::Number(_)),
                    "String" => matches!(val, Value::String(_)),
                    "Boolean" => matches!(val, Value::Boolean(_)),
                    "Object" => matches!(val, Value::Object(_)),
                    "Array" => matches!(val, Value::Array(_)),
                    "Null" => matches!(val, Value::Null),
                    "Any" => true,
                    _ => {
                        // Check particle _class field.
                        let expected_class = if let Some(dot_pos) = name.rfind('.') {
                            &name[dot_pos + 1..]
                        } else {
                            name.as_str()
                        };
                        match val {
                            Value::Object(fields) => fields
                                .get("_class")
                                .and_then(|v| {
                                    if let Value::String(s) = v.as_ref() {
                                        Some(s.as_str())
                                    } else {
                                        None
                                    }
                                })
                                .map_or(false, |cls| cls == expected_class),
                            _ => false,
                        }
                    }
                }
            }
            TypeExpr::Literal(s) => matches!(val, Value::String(v) if v == s),
            TypeExpr::Union(variants) => variants
                .iter()
                .any(|v| self.value_matches_type_expr(val, v, seen)),
            TypeExpr::Intersection(variants) => variants
                .iter()
                .all(|v| self.value_matches_type_expr(val, v, seen)),
        }
    }

    /// Execute a handler invocation.
    fn exec_handler_invoke(
        &mut self,
        particle_val: &Rc<Value>,
        target: &HandlerTarget,
    ) -> Result<Rc<Value>, String> {
        let class_name = match particle_val.as_ref() {
            Value::Object(fields) => match fields.get("_class") {
                Some(class_val) => match class_val.as_ref() {
                    Value::String(s) => s.clone(),
                    _ => return Err("Particle _class field is not a string".to_string()),
                },
                None => {
                    return Err(
                        "Cannot dispatch non-particle value (missing _class)".to_string()
                    )
                }
            },
            _ => return Err("Cannot dispatch non-object value as particle".to_string()),
        };

        // Core handlers are a small, fixed, compiled-in set (not user-extensible
        // like `.so`/`.wasm` handlers), so they bypass the body/native dispatch
        // below entirely.
        if matches!(target, HandlerTarget::Core) {
            return dispatch_core_handler(&class_name, particle_val);
        }

        let handler_bodies: Vec<Vec<Spanned<Statement>>> = match target {
            HandlerTarget::This => self
                .env
                .get_handler(&class_name)
                .cloned()
                .into_iter()
                .collect(),
            HandlerTarget::ModuleAlias(alias) => self
                .env
                .get_handler(&format!("{}.{}", alias, class_name))
                .cloned()
                .into_iter()
                .collect(),
            HandlerTarget::Base => {
                self.env.get_handlers_outside_current_scope(&class_name)
            }
            HandlerTarget::Core => unreachable!("handled by the early return above"),
        };

        let native_handlers: Vec<crate::native_module::NativeFnPtr> = match target {
            HandlerTarget::This => self
                .env
                .get_native_handler(&class_name)
                .cloned()
                .into_iter()
                .collect(),
            HandlerTarget::ModuleAlias(alias) => self
                .env
                .get_native_handler(&format!("{}.{}", alias, class_name))
                .cloned()
                .into_iter()
                .collect(),
            HandlerTarget::Base => self
                .env
                .get_native_handlers_outside_current_scope(&class_name),
            HandlerTarget::Core => unreachable!("handled by the early return above"),
        };

        if handler_bodies.is_empty() && native_handlers.is_empty() {
            return Ok(Value::null());
        }

        let mut last_result = Value::null();

        for handler_body in handler_bodies {
            self.env.push_scope();
            self.in_handler_depth += 1;

            if let Value::Object(fields) = particle_val.as_ref() {
                for (name, val) in fields {
                    if name != "_class" && name != "_created" {
                        self.env.define(name.clone(), Rc::clone(val));
                    }
                }
            }

            let mut exec_error: Option<String> = None;
            for stmt in handler_body {
                self.current_span = Some(stmt.span.clone());
                if let Err(e) = self.exec_statement(stmt.node) {
                    exec_error = Some(e);
                    break;
                }
                if self.handler_return_value.is_some() {
                    break;
                }
            }

            self.in_handler_depth -= 1;
            self.env.pop_scope();

            if let Some(e) = exec_error {
                return Err(e);
            }

            last_result = self
                .handler_return_value
                .take()
                .unwrap_or_else(|| Value::null());
        }

        for native_fn in native_handlers {
            let result = (native_fn.0)(vec![Rc::clone(particle_val)])?;
            last_result = result;
        }

        Ok(last_result)
    }

    // -----------------------------------------------------------------------
    // Native emission draining
    // -----------------------------------------------------------------------

    fn drain_native_emissions(&mut self) -> Result<(), String> {
        let mut emitted_particles: Vec<Rc<Value>> = Vec::new();
        for queue in &self.emit_queues {
            if let Ok(mut q) = queue.lock() {
                while let Some(emitted_val) = q.pop_front() {
                    emitted_particles.push(emitted_val.to_value());
                }
            }
        }

        for particle in emitted_particles {
            if let Value::Object(fields) = particle.as_ref() {
                if let Some(class) = fields.get("_class") {
                    if let Value::String(s) = class.as_ref() {
                        if s == "__KeepAlive" {
                            self.keep_alive = true;
                            continue;
                        }
                    }
                    self.exec_handler_invoke(&particle, &HandlerTarget::This)?;
                }
            }
        }

        Ok(())
    }

    /// Evaluate an expression, returning a heap-allocated value.
    fn eval_expr(&mut self, expr: Expression) -> Result<Rc<Value>, String> {
        match expr {
            Expression::Number(n) => Ok(Value::number(n)),
            Expression::String(s) => Ok(Value::string(s)),
            Expression::Boolean(b) => Ok(Value::boolean(b)),
            Expression::Null => Ok(Value::null()),
            Expression::Identifier(name) => self
                .env
                .get(&name)
                .ok_or_else(|| format!("Undefined variable '{}'", name)),
            Expression::Object { spread, fields } => {
                let mut map = HashMap::new();
                if let Some(source_expr) = spread {
                    let source_val = self.eval_expr(*source_expr)?;
                    match source_val.as_ref() {
                        Value::Object(src_fields) => {
                            for (k, v) in src_fields {
                                map.insert(k.clone(), v.clone());
                            }
                        }
                        other => {
                            return Err(format!(
                                "Spread source must be an object, got {}",
                                other.type_name()
                            ))
                        }
                    }
                }
                for field in fields {
                    match field {
                        ObjectField::Static(name, expr) => {
                            let val = self.eval_expr(expr)?;
                            map.insert(name, val);
                        }
                        ObjectField::Computed(key_expr, val_expr) => {
                            let key = self.eval_expr(key_expr)?;
                            let key_str = match key.as_ref() {
                                Value::String(s) => s.clone(),
                                Value::Number(n) => n.to_string(),
                                _ => return Err(format!(
                                    "Computed property key must be a String or Number, found {}",
                                    key.type_name()
                                )),
                            };
                            let val = self.eval_expr(val_expr)?;
                            map.insert(key_str, val);
                        }
                    }
                }
                Ok(Value::object(map))
            }
            Expression::Particle {
                qualifier,
                class_name,
                spread,
                fields,
            } => {
                let type_key = match &qualifier {
                    Some(q) => format!("{}.{}", q, class_name),
                    None => class_name.clone(),
                };

                let type_def = self.env.get_type(&type_key).cloned();

                let mut spread_map: HashMap<String, Rc<Value>> = HashMap::new();
                if let Some(source_expr) = spread {
                    let source_val = self.eval_expr(*source_expr)?;
                    match source_val.as_ref() {
                        Value::Object(src_fields) => {
                            for (k, v) in src_fields {
                                if k != "_class" && k != "_created" {
                                    spread_map.insert(k.clone(), v.clone());
                                }
                            }
                        }
                        other => {
                            return Err(format!(
                                "Spread source must be an object, got {}",
                                other.type_name()
                            ))
                        }
                    }
                }

                if let Some(ref schema_fields) = type_def {
                    let provided: std::collections::HashSet<String> =
                        fields.iter().filter_map(|f| match f {
                            ObjectField::Static(n, _) => Some(n.clone()),
                            ObjectField::Computed(_, _) => None,
                        }).collect();
                    for fc in schema_fields {
                        if !fc.optional
                            && !provided.contains(fc.name.as_str())
                            && !spread_map.contains_key(&fc.name)
                        {
                            return Err(format!(
                                "Missing field '{}' for type '{}'",
                                fc.name, type_key
                            ));
                        }
                    }
                    let schema_names: std::collections::HashSet<&str> =
                        schema_fields.iter().map(|fc| fc.name.as_str()).collect();
                    for f in &fields {
                        if let ObjectField::Static(pf_name, _) = f {
                            if !schema_names.contains(pf_name.as_str()) {
                                return Err(format!(
                                    "Unknown field '{}' for type '{}'",
                                    pf_name, type_key
                                ));
                            }
                        }
                    }

                    let mut map = HashMap::new();
                    map.insert("_class".to_string(), Value::string(class_name.clone()));
                    map.insert("_created".to_string(), Value::number(0.0));

                    for (k, v) in &spread_map {
                        map.insert(k.clone(), v.clone());
                    }

                    for field in fields {
                        match field {
                            ObjectField::Static(name, expr) => {
                                let val = self.eval_expr(expr)?;
                                if let Some(fc) = schema_fields.iter().find(|fc| fc.name == name) {
                                    let expected_type = fc.primary_type();
                                    let ok_null = fc.optional && matches!(val.as_ref(), Value::Null);
                                    if !ok_null
                                        && !self.value_matches_type_expr(
                                            &val,
                                            &expected_type,
                                            &mut Vec::new(),
                                        )
                                    {
                                        return Err(format!(
                                            "Type mismatch for field '{}' of '{}': expected {}, got {}",
                                            name,
                                            type_key,
                                            expected_type,
                                            val.type_name()
                                        ));
                                    }
                                }
                                map.insert(name, val);
                            }
                            ObjectField::Computed(key_expr, val_expr) => {
                                let key = self.eval_expr(key_expr)?;
                                let key_str = match key.as_ref() {
                                    Value::String(s) => s.clone(),
                                    Value::Number(n) => n.to_string(),
                                    _ => return Err(format!(
                                    "Computed property key must be a String or Number, found {}",
                                    key.type_name()
                                )),
                                };
                                let val = self.eval_expr(val_expr)?;
                                map.insert(key_str, val);
                            }
                        }
                    }

                    for fc in schema_fields {
                        if fc.optional && !map.contains_key(&fc.name) {
                            map.insert(fc.name.clone(), Value::null());
                        }
                    }

                    Ok(Value::object(map))
                } else {
                    let mut map = HashMap::new();
                    map.insert("_class".to_string(), Value::string(class_name));
                    map.insert("_created".to_string(), Value::number(0.0));
                    for (k, v) in &spread_map {
                        map.insert(k.clone(), v.clone());
                    }
                    for field in fields {
                        match field {
                            ObjectField::Static(name, expr) => {
                                let val = self.eval_expr(expr)?;
                                map.insert(name, val);
                            }
                            ObjectField::Computed(key_expr, val_expr) => {
                                let key = self.eval_expr(key_expr)?;
                                let key_str = match key.as_ref() {
                                    Value::String(s) => s.clone(),
                                    Value::Number(n) => n.to_string(),
                                    _ => return Err(format!(
                                    "Computed property key must be a String or Number, found {}",
                                    key.type_name()
                                )),
                                };
                                let val = self.eval_expr(val_expr)?;
                                map.insert(key_str, val);
                            }
                        }
                    }
                    Ok(Value::object(map))
                }
            }
            Expression::PropertyAccess(recv, field) => {
                let obj = self.eval_expr(*recv)?;
                match obj.as_ref() {
                    Value::Object(fields) => Ok(fields
                        .get(&field)
                        .cloned()
                        .unwrap_or_else(|| Value::null())),
                    other => Err(format!(
                        "Cannot access property '{}' on non-object value: {}",
                        field, other
                    )),
                }
            }
            Expression::ArrayLiteral(elements) => {
                let mut vals = Vec::new();
                for elem in elements {
                    vals.push(self.eval_expr(elem)?);
                }
                Ok(Value::array(vals))
            }
            Expression::IndexAccess { receiver, index } => {
                let recv_val = self.eval_expr(*receiver)?;
                let idx_val = self.eval_expr(*index)?;
                match (recv_val.as_ref(), idx_val.as_ref()) {
                    (Value::Array(elements), Value::Number(n)) => {
                        let i = *n as usize;
                        if (i as f64) != *n || *n < 0.0 {
                            return Ok(Value::null());
                        }
                        Ok(elements.get(i).cloned().unwrap_or_else(|| Value::null()))
                    }
                    (Value::Array(_), _) => Err(format!(
                        "Array index must be a Number, found {}",
                        idx_val.type_name()
                    )),
                    _ => Err(format!(
                        "Index access requires an Array value, found {}",
                        recv_val.type_name()
                    )),
                }
            }
            Expression::InterpolatedString(parts) => {
                let mut result = String::new();
                for part in parts {
                    match part {
                        crate::ast::StringPart::Literal(s) => result.push_str(&s),
                        crate::ast::StringPart::Variable(name) => {
                            let val = self.env.get(&name).ok_or_else(|| {
                                format!(
                                    "Undefined variable '{}' in string interpolation",
                                    name
                                )
                            })?;
                            match val.as_ref() {
                                Value::String(s) => result.push_str(s),
                                Value::Number(n) => result.push_str(&n.to_string()),
                                Value::Boolean(b) => result.push_str(&b.to_string()),
                                Value::Null => result.push_str("Null"),
                                _ => result.push_str(&val.to_string()),
                            }
                        }
                    }
                }
                Ok(Value::string(result))
            }
            Expression::Binary {
                left,
                op,
                right,
            } => {
                match op {
                    BinaryOp::And => {
                        let left_val = self.eval_expr(*left)?;
                        match left_val.as_ref() {
                            Value::Boolean(false) => return Ok(Value::boolean(false)),
                            Value::Boolean(true) => {
                                let right_val = self.eval_expr(*right)?;
                                match right_val.as_ref() {
                                    Value::Boolean(_) => return Ok(right_val),
                                    _ => {
                                        return Err(
                                            "Right operand of 'and' must be a Boolean"
                                                .to_string(),
                                        )
                                    }
                                }
                            }
                            _ => {
                                return Err(
                                    "Left operand of 'and' must be a Boolean".to_string()
                                )
                            }
                        }
                    }
                    BinaryOp::Or => {
                        let left_val = self.eval_expr(*left)?;
                        match left_val.as_ref() {
                            Value::Boolean(true) => return Ok(Value::boolean(true)),
                            Value::Boolean(false) => {
                                let right_val = self.eval_expr(*right)?;
                                match right_val.as_ref() {
                                    Value::Boolean(_) => return Ok(right_val),
                                    _ => {
                                        return Err(
                                            "Right operand of 'or' must be a Boolean"
                                                .to_string(),
                                        )
                                    }
                                }
                            }
                            _ => {
                                return Err(
                                    "Left operand of 'or' must be a Boolean".to_string()
                                )
                            }
                        }
                    }
                    _ => {
                        let left_val = self.eval_expr(*left)?;
                        let right_val = self.eval_expr(*right)?;
                        self.eval_binary(op, &left_val, &right_val)
                    }
                }
            }
            Expression::Unary { op, operand } => {
                let val = self.eval_expr(*operand)?;
                match op {
                    UnaryOp::Not => match val.as_ref() {
                        Value::Boolean(b) => Ok(Value::boolean(!b)),
                        _ => Err(format!(
                            "Operand of 'not' must be a Boolean, found {}",
                            val.type_name()
                        )),
                    },
                    UnaryOp::Negate => match val.as_ref() {
                        Value::Number(n) => Ok(Value::number(-n)),
                        _ => Err(format!(
                            "Operand of '-' must be a Number, found {}",
                            val.type_name()
                        )),
                    },
                }
            }
            Expression::TypeCheck {
                expr,
                type_expr,
                negated,
            } => {
                let val = self.eval_expr(*expr)?;
                let matches =
                    self.value_matches_type_expr(&val, &type_expr, &mut Vec::new());
                let result = if negated { !matches } else { matches };
                Ok(Value::boolean(result))
            }
        }
    }

    /// Evaluate a binary operation.
    fn eval_binary(
        &self,
        op: BinaryOp,
        left: &Value,
        right: &Value,
    ) -> Result<Rc<Value>, String> {
        match op {
            BinaryOp::Equal => Ok(Value::boolean(values_equal(left, right))),
            BinaryOp::NotEqual => Ok(Value::boolean(!values_equal(left, right))),
            BinaryOp::Add => match (left, right) {
                (Value::String(a), Value::String(b)) => {
                    Ok(Value::string(format!("{}{}", a, b)))
                }
                (Value::String(a), other) => Ok(Value::string(format!("{}{}", a, other))),
                (other, Value::String(b)) => Ok(Value::string(format!("{}{}", other, b))),
                (Value::Object(a), Value::Object(b)) => {
                    let mut result = a.clone();
                    for (k, v) in b {
                        result.insert(k.clone(), v.clone());
                    }
                    Ok(Value::object(result))
                }
                (Value::Array(a), Value::Array(b)) => {
                    let mut result = a.clone();
                    result.extend(b.iter().cloned());
                    Ok(Value::array(result))
                }
                (Value::Array(a), val) => {
                    let mut result = a.clone();
                    result.push(Rc::new(val.clone()));
                    Ok(Value::array(result))
                }
                (val, Value::Array(b)) => {
                    let mut result = vec![Rc::new(val.clone())];
                    result.extend(b.iter().cloned());
                    Ok(Value::array(result))
                }
                (Value::Number(a), Value::Number(b)) => Ok(Value::number(a + b)),
                _ => Err(format!(
                    "Cannot use '+' with {} and {}",
                    left.type_name(),
                    right.type_name()
                )),
            },
            BinaryOp::Sub => match (left, right) {
                (Value::Number(a), Value::Number(b)) => Ok(Value::number(a - b)),
                _ => Err(format!(
                    "Cannot use '-' with {} and {}",
                    left.type_name(),
                    right.type_name()
                )),
            },
            BinaryOp::Mul => match (left, right) {
                (Value::Number(a), Value::Number(b)) => Ok(Value::number(a * b)),
                _ => Err(format!(
                    "Cannot use '*' with {} and {}",
                    left.type_name(),
                    right.type_name()
                )),
            },
            BinaryOp::Div => match (left, right) {
                (Value::Number(a), Value::Number(b)) => {
                    if *b == 0.0 {
                        Err("Division by zero".to_string())
                    } else {
                        Ok(Value::number(a / b))
                    }
                }
                _ => Err(format!(
                    "Cannot use '/' with {} and {}",
                    left.type_name(),
                    right.type_name()
                )),
            },
            BinaryOp::Less => match (left, right) {
                (Value::Number(a), Value::Number(b)) => Ok(Value::boolean(a < b)),
                _ => Err(format!(
                    "Cannot use '<' with {} and {}",
                    left.type_name(),
                    right.type_name()
                )),
            },
            BinaryOp::Greater => match (left, right) {
                (Value::Number(a), Value::Number(b)) => Ok(Value::boolean(a > b)),
                _ => Err(format!(
                    "Cannot use '>' with {} and {}",
                    left.type_name(),
                    right.type_name()
                )),
            },
            BinaryOp::LessEqual => match (left, right) {
                (Value::Number(a), Value::Number(b)) => Ok(Value::boolean(a <= b)),
                _ => Err(format!(
                    "Cannot use '≤' with {} and {}",
                    left.type_name(),
                    right.type_name()
                )),
            },
            BinaryOp::GreaterEqual => match (left, right) {
                (Value::Number(a), Value::Number(b)) => Ok(Value::boolean(a >= b)),
                _ => Err(format!(
                    "Cannot use '≥' with {} and {}",
                    left.type_name(),
                    right.type_name()
                )),
            },
            BinaryOp::And | BinaryOp::Or => unreachable!(),
        }
    }
}

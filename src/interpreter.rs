use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{BinOp, EmitTarget, Expr, NativeFormat, Program, Stmt, UnOp};
#[cfg(feature = "native-modules")]
use crate::native::NativeModule;
use crate::value::Value;

/// A name -> Value binding table, scoped for `if`/`let` (see memory
/// `new-code-if-scoping` and `new-code-let-keyword`): a stack of maps,
/// innermost last. `declare` (`let`) always writes to the current
/// (innermost) scope, shadowing any outer same-named binding. `assign`
/// (bare `name = expr`) searches from innermost to outermost for an
/// *existing* binding and updates it in place — an error if there isn't
/// one anywhere. Rebinding a name to a Value of a different variant is not
/// an error — variables are untyped, only Values are.
#[derive(Debug)]
pub struct Environment {
    scopes: Vec<HashMap<String, Value>>,
    /// First-assignment order of names in the *outermost* scope only — the
    /// only scope whose bindings ever get dumped (see `iter_in_order`); an
    /// `if`-local binding never appears here even if the `if` runs.
    order: Vec<String>,
    /// Linked native modules, by alias — a separate namespace from
    /// `scopes`, not a `Value`: this language has no function-value kind a
    /// handler could be represented as, so a native module is only ever
    /// reachable via `emit ... to <alias>`, never as an ordinary binding.
    /// Always top-level, like `Stmt::ImportNative` itself.
    #[cfg(feature = "native-modules")]
    native_modules: HashMap<String, NativeModule>,
}

impl Default for Environment {
    fn default() -> Self {
        Environment {
            scopes: vec![HashMap::new()],
            order: Vec::new(),
            #[cfg(feature = "native-modules")]
            native_modules: HashMap::new(),
        }
    }
}

impl Environment {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    /// `let name = value` — always a new binding in the current scope,
    /// even if `name` already exists further out (shadowing) or even in
    /// this exact scope (re-`let`, just rebinds).
    fn declare(&mut self, name: String, value: Value) {
        if self.scopes.len() == 1 && !self.scopes[0].contains_key(&name) {
            self.order.push(name.clone());
        }
        self.scopes.last_mut().unwrap().insert(name, value);
    }

    /// Bare `name = value` — reassigns an existing binding, found by
    /// searching outward; errors if `name` isn't bound anywhere.
    fn assign(&mut self, name: &str, value: Value) -> Result<(), String> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(slot) = scope.get_mut(name) {
                *slot = value;
                return Ok(());
            }
        }
        Err(format!(
            "undefined variable '{name}' (use 'let {name} = ...' to declare it)"
        ))
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Bindings in first-assignment order, for stable, deterministic output
    /// — outermost scope only (see `order`'s doc comment).
    pub fn iter_in_order(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.order.iter().map(|name| {
            (
                name,
                self.scopes[0]
                    .get(name)
                    .expect("outermost binding must still exist"),
            )
        })
    }
}

pub fn run(program: &Program) -> Result<Environment, String> {
    let mut env = Environment::default();
    for stmt in &program.statements {
        // Can only ever be `Flow::Normal` out here: the parser rejects a
        // `break` that isn't inside a loop, so nothing can propagate one up
        // to the top level.
        exec(stmt, &mut env)?;
    }
    Ok(env)
}

/// Whether a statement finished normally or hit a `break` that the innermost
/// enclosing `Stmt::Loop` still has to act on. Nested `if`/block bodies just
/// pass it straight through — a `break` inside an `if` inside a loop breaks
/// the loop, not the `if`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Flow {
    Normal,
    Break,
}

/// Runs `body` in a *new scope*, stopping early on a `break`. Pops the scope
/// on every path, error included — hence the explicit `result` binding
/// rather than `?` mid-function.
fn exec_scoped_body(body: &[Stmt], env: &mut Environment) -> Result<Flow, String> {
    env.push_scope();
    let result = exec_body(body, env);
    env.pop_scope();
    result
}

fn exec_body(body: &[Stmt], env: &mut Environment) -> Result<Flow, String> {
    for stmt in body {
        if exec(stmt, env)? == Flow::Break {
            return Ok(Flow::Break);
        }
    }
    Ok(Flow::Normal)
}

fn exec(stmt: &Stmt, env: &mut Environment) -> Result<Flow, String> {
    match stmt {
        // `exported` is a module-boundary marker consumed by `loader.rs`; a
        // declaration behaves identically either way.
        Stmt::Let { name, value, .. } => {
            let v = eval(value, env)?;
            env.declare(name.clone(), v);
            Ok(Flow::Normal)
        }
        Stmt::Link { path, .. } => Err(format!(
            "internal error: link \"{path}\" reached the interpreter unresolved"
        )),
        Stmt::ImportNative { alias, path, format } => {
            if let NativeFormat::Static { .. } = format {
                return Err(format!(
                    "link \"{path}\": .a modules only work with 'code build', not 'code run' \
                     — see docs/todo/native-module-linking.md"
                ));
            }
            #[cfg(feature = "native-modules")]
            {
                let module = NativeModule::open(path)?;
                // The module's exported variables (constants) become an
                // object bound under `alias`, so `alias.name` is ordinary
                // field access — the same binding `Import`'s alias uses. A
                // module with no `code_module_vars` export yields an empty
                // object. The module itself is kept in a separate namespace
                // for `emit ... to <alias>` dispatch.
                let vars = module.vars()?;
                env.declare(alias.clone(), Value::Object(Rc::new(vars)));
                env.native_modules.insert(alias.clone(), module);
                Ok(Flow::Normal)
            }
            #[cfg(not(feature = "native-modules"))]
            {
                let _ = (alias, path, format);
                Err("native modules aren't supported in this build".to_string())
            }
        }
        Stmt::Import {
            alias,
            body,
            exports,
        } => {
            // Produce the exported name/value pairs, then bind them. The two
            // halves are kept separate because a native module would supply
            // the pairs from a descriptor instead of from a body, and reuse
            // the binding half unchanged (see `ast::Stmt::Import`).
            env.push_scope();
            let result = exec_body(body, env);
            let pairs = result.and_then(|_| {
                exports
                    .iter()
                    .map(|name| {
                        env.get(name)
                            .cloned()
                            .map(|value| (name.clone(), value))
                            .ok_or_else(|| format!("module exports '{name}' but never defines it"))
                    })
                    .collect::<Result<Vec<_>, _>>()
            });
            env.pop_scope();
            let pairs = pairs?;

            match alias {
                Some(alias) => env.declare(alias.clone(), Value::Object(Rc::new(pairs))),
                None => {
                    for (name, value) in pairs {
                        if env.get(&name).is_some() {
                            return Err(format!(
                                "linking would redefine '{name}' — rename it, or use \
                                 'link ... as <name>' to keep the module's names apart"
                            ));
                        }
                        env.declare(name, value);
                    }
                }
            }
            Ok(Flow::Normal)
        }
        Stmt::Assign { name, value } => {
            let v = eval(value, env)?;
            env.assign(name, v)?;
            Ok(Flow::Normal)
        }
        Stmt::Assert(expr) => match eval(expr, env)? {
            Value::Bool(true) => Ok(Flow::Normal),
            Value::Bool(false) => Err("assertion failed".to_string()),
            v => Err(format!(
                "assert requires a boolean, found a {}",
                type_name(&v)
            )),
        },
        Stmt::If { condition, body } => match eval(condition, env)? {
            Value::Bool(true) => exec_scoped_body(body, env),
            Value::Bool(false) => Ok(Flow::Normal),
            v => Err(format!("if requires a boolean, found a {}", type_name(&v))),
        },
        Stmt::Block(body) => exec_scoped_body(body, env),
        Stmt::Loop {
            var,
            index,
            iterable,
            body,
        } => {
            // Evaluated once, up front. Holding the `Rc` here is what makes
            // that a real snapshot: the body may reassign whatever binding
            // the array came from without disturbing the iteration (and
            // since no value is ever mutated in place, the snapshot can't
            // go stale either way — see memory `new-code-memory-management`).
            // Matched by reference, not by move: `Value` has a manual `Drop`
            // (see value.rs), and Rust forbids moving a field out of such a
            // type. `Rc::clone` is the O(1) equivalent here anyway.
            let evaluated = eval(iterable, env)?;
            let items = match &evaluated {
                Value::Array(items) => Rc::clone(items),
                v => return Err(format!("loop requires an array, found a {}", type_name(v))),
            };
            for (i, item) in items.iter().enumerate() {
                env.push_scope();
                env.declare(var.clone(), item.clone());
                if let Some(index) = index {
                    env.declare(index.clone(), Value::Number(i as f64));
                }
                let result = exec_body(body, env);
                env.pop_scope();
                if result? == Flow::Break {
                    break;
                }
            }
            Ok(Flow::Normal)
        }
        Stmt::Emit {
            particle,
            target,
            result,
        } => {
            let value = eval(particle, env)?;
            let output = match target {
                EmitTarget::Core => dispatch_core(&value)?,
                EmitTarget::Module(alias) => {
                    #[cfg(feature = "native-modules")]
                    {
                        let module = env
                            .native_modules
                            .get(alias)
                            .ok_or_else(|| format!("no linked native module named '{alias}'"))?;
                        module.dispatch(&value)?
                    }
                    #[cfg(not(feature = "native-modules"))]
                    {
                        let _ = alias;
                        return Err("native modules aren't supported in this build".to_string());
                    }
                }
            };
            if let Some(name) = result {
                env.declare(name.clone(), output);
            }
            Ok(Flow::Normal)
        }
        Stmt::Break => Ok(Flow::Break),
    }
}

fn eval(expr: &Expr, env: &Environment) -> Result<Value, String> {
    match expr {
        Expr::Number(n) => Ok(Value::Number(*n)),
        Expr::Str(s) => Ok(Value::Str(Rc::from(s.as_str()))),
        Expr::Bool(b) => Ok(Value::Bool(*b)),
        Expr::Null => Ok(Value::Null),
        // Cheap regardless of size: `Value::clone` on Str/Array/Object is
        // just an Rc refcount bump, not a deep copy (see value.rs).
        Expr::Ident(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| format!("undefined variable '{name}'")),
        Expr::Array(items) => {
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(eval(item, env)?);
            }
            Ok(Value::Array(Rc::new(values)))
        }
        Expr::Object(fields) => {
            let mut values = Vec::with_capacity(fields.len());
            for (key, value) in fields {
                values.push((key.clone(), eval(value, env)?));
            }
            Ok(Value::Object(Rc::new(values)))
        }
        // Invalid access (non-object, missing field / non-array, bad index)
        // returns Null rather than erroring — decided 2026-08-21, permissive
        // like JS, unlike undefined-variable reads which still error.
        Expr::Field(obj, field) => {
            let v = eval(obj, env)?;
            Ok(match &v {
                Value::Object(fields) => fields
                    .iter()
                    .find(|(k, _)| k == field)
                    .map(|(_, v)| v.clone())
                    .unwrap_or(Value::Null),
                _ => Value::Null,
            })
        }
        Expr::Index(arr, index) => {
            let v = eval(arr, env)?;
            let i = eval(index, env)?;
            Ok(match (&v, &i) {
                (Value::Array(items), Value::Number(n)) if n.fract() == 0.0 && *n >= 0.0 => {
                    items.get(*n as usize).cloned().unwrap_or(Value::Null)
                }
                _ => Value::Null,
            })
        }
        Expr::Unary(op, e) => {
            let v = eval(e, env)?;
            match (op, v) {
                (UnOp::Neg, Value::Number(n)) => Ok(Value::Number(-n)),
                (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
                (UnOp::Neg, v) => Err(format!("cannot negate a {}", type_name(&v))),
                (UnOp::Not, v) => Err(format!(
                    "'not' requires a boolean, found a {}",
                    type_name(&v)
                )),
            }
        }
        // `and`/`or` short-circuit: the right side is only evaluated (and
        // only needs to be a bool) when the left side didn't already
        // determine the result.
        Expr::Binary(lhs, BinOp::And, rhs) => match eval(lhs, env)? {
            Value::Bool(false) => Ok(Value::Bool(false)),
            Value::Bool(true) => require_bool(eval(rhs, env)?, "and"),
            v => Err(format!(
                "'and' requires booleans, found a {}",
                type_name(&v)
            )),
        },
        Expr::Binary(lhs, BinOp::Or, rhs) => match eval(lhs, env)? {
            Value::Bool(true) => Ok(Value::Bool(true)),
            Value::Bool(false) => require_bool(eval(rhs, env)?, "or"),
            v => Err(format!("'or' requires booleans, found a {}", type_name(&v))),
        },
        // Equality is well-defined for any two values, including mismatched
        // kinds (simply `false`, never an error) — `Value`'s derived
        // `PartialEq` already does exactly that.
        Expr::Binary(lhs, BinOp::Eq, rhs) => Ok(Value::Bool(eval(lhs, env)? == eval(rhs, env)?)),
        Expr::Binary(lhs, BinOp::Ne, rhs) => Ok(Value::Bool(eval(lhs, env)? != eval(rhs, env)?)),
        Expr::Binary(lhs, op, rhs) => {
            let l = eval(lhs, env)?;
            let r = eval(rhs, env)?;
            apply_binop(*op, l, r)
        }
    }
}

/// Runs the compiled-in "core" handler named by `particle`'s own `"_class"`
/// field. Must match `runtime.c`'s `code_core_dispatch` exactly — same
/// handler set, same operand-type rules, same error wording where it's worth
/// keeping in sync.
fn dispatch_core(particle: &Value) -> Result<Value, String> {
    let Value::Object(fields) = particle else {
        return Err(format!(
            "emit requires a particle (an object with a \"_class\" field), found a {}",
            type_name(particle)
        ));
    };
    let class = match fields.iter().find(|(k, _)| k == "_class") {
        Some((_, Value::Str(class))) => class,
        _ => {
            return Err("emit requires a particle (an object with a \"_class\" field)".to_string())
        }
    };
    match class.as_ref() {
        "Length" => match fields.iter().find(|(k, _)| k == "value") {
            Some((_, Value::Array(items))) => Ok(core_result("LengthResult", items.len() as f64)),
            Some((_, Value::Str(s))) => Ok(core_result("LengthResult", s.len() as f64)),
            Some((_, v)) => Err(format!(
                "Length requires an array or string 'value', found a {}",
                type_name(v)
            )),
            None => Err("Length { \"value\": ... } requires a 'value' field".to_string()),
        },
        other => Err(format!("unknown core handler '{other}'")),
    }
}

/// `{ "_class": class_name, "value": n }` — the shape every core handler's
/// result takes, matching the old language's `<Name>Result` convention:
/// what goes into `emit` is a particle, so what comes back out is one too,
/// not a bare scalar. Must match `runtime.c`'s `code_make_result` exactly.
fn core_result(class_name: &str, value: f64) -> Value {
    Value::Object(Rc::new(vec![
        ("_class".to_string(), Value::Str(Rc::from(class_name))),
        ("value".to_string(), Value::Number(value)),
    ]))
}

fn require_bool(v: Value, op: &str) -> Result<Value, String> {
    match v {
        Value::Bool(_) => Ok(v),
        v => Err(format!(
            "'{op}' requires booleans, found a {}",
            type_name(&v)
        )),
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Number(_) => "number",
        Value::Str(_) => "string",
        Value::Bool(_) => "boolean",
        Value::Null => "null",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// `Add`/`Sub`/`Mul`/`Div`/`Lt`/`Gt`/`Le`/`Ge` — see `ast::BinOp`'s doc
/// comment for the exact operand-type rules this implements. `And`/`Or`/
/// `Eq`/`Ne` are handled directly in `eval` (short-circuiting / always-
/// defined, respectively) and never reach here.
fn apply_binop(op: BinOp, l: Value, r: Value) -> Result<Value, String> {
    use Value::*;
    let result = match (op, &l, &r) {
        (BinOp::Add, Number(a), Number(b)) => Some(Number(a + b)),
        (BinOp::Add, Str(a), Str(b)) => Some(Str(Rc::from(format!("{a}{b}").as_str()))),
        (BinOp::Add, Array(a), Array(b)) => {
            let mut items = (**a).clone();
            items.extend(b.iter().cloned());
            Some(Array(Rc::new(items)))
        }
        (BinOp::Sub, Number(a), Number(b)) => Some(Number(a - b)),
        (BinOp::Mul, Number(a), Number(b)) => Some(Number(a * b)),
        (BinOp::Div, Number(a), Number(b)) => {
            if *b == 0.0 {
                return Err("division by zero".to_string());
            }
            Some(Number(a / b))
        }
        (BinOp::Lt, Number(a), Number(b)) => Some(Bool(a < b)),
        (BinOp::Gt, Number(a), Number(b)) => Some(Bool(a > b)),
        (BinOp::Le, Number(a), Number(b)) => Some(Bool(a <= b)),
        (BinOp::Ge, Number(a), Number(b)) => Some(Bool(a >= b)),
        _ => None,
    };
    result.ok_or_else(|| {
        format!(
            "cannot apply {op:?} to a {} and a {}",
            type_name(&l),
            type_name(&r)
        )
    })
}

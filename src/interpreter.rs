use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::ast::{BinOp, EmitTarget, Expr, NativeFormat, Program, Stmt, UnOp};
#[cfg(feature = "native-modules")]
use crate::native::NativeModule;
use crate::value::Value;

/// A linked module's live dispatch entry point — what `emit ... to <alias>`
/// calls. Closure-based rather than a trait `Environment` is generic over:
/// a `.so` module (`native.rs`, `NativeFormat::Dynamic`) and a
/// `crates/code-wasm` JS callback (`NativeFormat::JsBridge`) wrap their very
/// different underlying mechanisms into the exact same shape, the way the
/// old language's `NativeFnPtr` let a `dlopen`'d `.so` and a `wasmi`-hosted
/// `.wasm` share one module type. Exported *variables*, unlike dispatch,
/// need no such abstraction — they're read once at `link` time and stored
/// as an ordinary `Value` binding (see `link_module`), the same for every
/// format.
pub type ModuleDispatch = Rc<dyn Fn(&Value) -> Result<Value, String>>;

/// A name -> Value binding table, scoped for `if`/`let` (see memory
/// `new-code-if-scoping` and `new-code-let-keyword`): a stack of maps,
/// innermost last. `declare` (`let`) always writes to the current
/// (innermost) scope, shadowing any outer same-named binding. `assign`
/// (bare `name = expr`) searches from innermost to outermost for an
/// *existing* binding and updates it in place — an error if there isn't
/// one anywhere. Rebinding a name to a Value of a different variant is not
/// an error — variables are untyped, only Values are.
pub struct Environment {
    scopes: Vec<HashMap<String, Value>>,
    /// First-assignment order of names in the *outermost* scope only — the
    /// only scope whose bindings ever get dumped (see `iter_in_order`); an
    /// `if`-local binding never appears here even if the `if` runs.
    order: Vec<String>,
    /// Linked modules' dispatch entry points, by alias — a separate
    /// namespace from `scopes`, not a `Value`: this language has no
    /// function-value kind a handler could be represented as, so a module is
    /// only ever reachable via `emit ... to <alias>`, never as an ordinary
    /// binding. Always top-level, like `Stmt::ImportNative` itself. Present
    /// unconditionally (not gated on `native-modules`): `crates/code-wasm`
    /// needs it too, with that feature off.
    modules: HashMap<String, ModuleDispatch>,
    /// Modules a host (`crates/code-wasm`) made available *before* the
    /// program started, by the name it registered them under — distinct from
    /// `modules`, which is keyed by whatever alias a `link "<name>" as
    /// <alias>` statement actually bound, and those two names can differ
    /// (`.so`'s `path` and `alias` are just as decoupled). A
    /// `NativeFormat::JsBridge` `ImportNative` looks its `path` up here and
    /// promotes it into `modules`/a binding under `alias` when it runs — see
    /// `provide_module` and that `exec` arm.
    available_modules: HashMap<String, (Value, ModuleDispatch)>,
}

/// Derived `Debug` doesn't work once a field holds a `dyn Fn` (no `Debug`
/// impl for closures) — written by hand instead, showing `modules`' keys
/// only, not the dispatchers themselves.
impl fmt::Debug for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Environment")
            .field("scopes", &self.scopes)
            .field("order", &self.order)
            .field("modules", &self.modules.keys().collect::<Vec<_>>())
            .field(
                "available_modules",
                &self.available_modules.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Default for Environment {
    fn default() -> Self {
        Environment {
            scopes: vec![HashMap::new()],
            order: Vec::new(),
            modules: HashMap::new(),
            available_modules: HashMap::new(),
        }
    }
}

impl Environment {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    /// Links a module under `alias`: binds `alias` to `vars` (an ordinary
    /// field-accessible value, exactly like `Import`'s alias — see
    /// `Stmt::ImportNative`'s doc comment) and registers `dispatch` as what
    /// `emit ... to <alias>` calls. What every module format's `exec` arm
    /// calls once it has resolved its own way (`.so` via `NativeModule::open`;
    /// `JsBridge` via `available_modules`, below) — the one place an alias
    /// actually becomes usable.
    pub fn link_module(&mut self, alias: &str, vars: Value, dispatch: ModuleDispatch) {
        self.declare(alias.to_string(), vars);
        self.modules.insert(alias.to_string(), dispatch);
    }

    /// Makes a module available under `name` for a *later* `link "<name>" as
    /// <alias>"` to promote into a real binding via `link_module` — what a
    /// host (`crates/code-wasm`) calls, once per JS-provided module, before
    /// the program itself runs at all. `name` is deliberately a separate
    /// namespace from any alias a script chooses: exactly like a `.so`'s
    /// file path and its `as` alias can differ, `link "mymath" as m"` is
    /// free to rename whatever the host called `"mymath"`.
    pub fn provide_module(&mut self, name: &str, vars: Value, dispatch: ModuleDispatch) {
        self.available_modules
            .insert(name.to_string(), (vars, dispatch));
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
    run_with(program, Environment::default())
}

/// Like `run`, but against a caller-supplied `Environment` rather than
/// always starting from `Environment::default()` — the hook
/// `crates/code-wasm` needs to pre-link its JS-callback modules
/// (`Environment::link_module`) before the program itself ever runs, since
/// a `JsBridge`-formatted `ImportNative` only ever checks an alias is
/// already present, never resolves one itself (see that arm below).
pub fn run_with(program: &Program, mut env: Environment) -> Result<Environment, String> {
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
    /// Like `Break`, but the enclosing loop starts its next iteration
    /// instead of stopping. Propagates outward through `if`/block bodies
    /// exactly the same way.
    Continue,
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
        let flow = exec(stmt, env)?;
        if flow != Flow::Normal {
            return Ok(flow);
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
        Stmt::ImportNative {
            alias,
            path,
            format,
        } => match format {
            NativeFormat::Static { .. } => Err(format!(
                "link \"{path}\": .a modules only work with 'code build', not 'code run' \
                 — see docs/todo/native-module-linking.md"
            )),
            // The host (`crates/code-wasm`) must have already called
            // `Environment::provide_module(path, ...)` before the program
            // started running at all — `path` is the name the host
            // registered it under, which `alias` (this `link ... as
            // <alias>`) may rename. See `ast::NativeFormat::JsBridge`.
            NativeFormat::JsBridge => {
                let (vars, dispatch) =
                    env.available_modules.get(path).cloned().ok_or_else(|| {
                        format!(
                            "link \"{path}\": no module named '{path}' was provided before running"
                        )
                    })?;
                env.link_module(alias, vars, dispatch);
                Ok(Flow::Normal)
            }
            NativeFormat::Dynamic => {
                #[cfg(feature = "native-modules")]
                {
                    let module = NativeModule::open(path)?;
                    // The module's exported variables (constants) become an
                    // object bound under `alias`, so `alias.name` is ordinary
                    // field access — the same binding `Import`'s alias uses.
                    // A module with no `code_module_vars` export yields an
                    // empty object.
                    let vars = module.vars()?;
                    let dispatch: ModuleDispatch = Rc::new(move |v| module.dispatch(v));
                    env.link_module(alias, Value::Object(Rc::new(vars)), dispatch);
                    Ok(Flow::Normal)
                }
                #[cfg(not(feature = "native-modules"))]
                {
                    let _ = (alias, path);
                    Err("native modules aren't supported in this build".to_string())
                }
            }
        },
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
        Stmt::Loop { over, result, body } => {
            // The accumulator is an ordinary binding in the scope *around*
            // the loop, created before the first iteration — which is what
            // makes it survive each iteration's scope and still be bound
            // afterwards, with no accumulator machinery at all. The body
            // updates it through the same `Stmt::Assign` as any other
            // reassignment (see `ast::LoopAccumulator`).
            if let Some(acc) = result {
                let init = eval(&acc.init, env)?;
                env.declare(acc.name.clone(), init);
            }

            match over {
                Some(over) => {
                    // Evaluated once, up front. Holding the `Rc` here is what
                    // makes that a real snapshot: the body may reassign
                    // whatever binding the array came from without disturbing
                    // the iteration (and since no value is ever mutated in
                    // place, the snapshot can't go stale either way — see
                    // memory `new-code-memory-management`). Matched by
                    // reference, not by move: `Value` has a manual `Drop`
                    // (see value.rs), and Rust forbids moving a field out of
                    // such a type. `Rc::clone` is the O(1) equivalent anyway.
                    let evaluated = eval(&over.iterable, env)?;
                    let items = match &evaluated {
                        Value::Array(items) => Rc::clone(items),
                        v => {
                            return Err(format!("loop requires an array, found a {}", type_name(v)))
                        }
                    };
                    for (i, item) in items.iter().enumerate() {
                        env.push_scope();
                        env.declare(over.var.clone(), item.clone());
                        if let Some(index) = &over.index {
                            env.declare(index.clone(), Value::Number(i as f64));
                        }
                        let flow = exec_body(body, env);
                        env.pop_scope();
                        // `Continue` needs no action beyond ending this
                        // iteration, which returning from the body already
                        // did — only `Break` changes what happens next.
                        if flow? == Flow::Break {
                            break;
                        }
                    }
                }
                // `loop { }` — nothing bounds this but `break`. See
                // `Stmt::Loop`'s doc comment on the guarantee that gives up.
                None => loop {
                    env.push_scope();
                    let flow = exec_body(body, env);
                    env.pop_scope();
                    if flow? == Flow::Break {
                        break;
                    }
                },
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
                    let dispatch = env
                        .modules
                        .get(alias)
                        .ok_or_else(|| format!("no linked module named '{alias}'"))?
                        .clone();
                    dispatch(&value)?
                }
            };
            if let Some(name) = result {
                env.declare(name.clone(), output);
            }
            Ok(Flow::Normal)
        }
        Stmt::Break => Ok(Flow::Break),
        Stmt::Continue => Ok(Flow::Continue),
    }
}

/// Evaluates a literal expression with no variables to resolve — the other
/// half of `parser::parse_expr`'s JSON-decoding trick: a JSON literal
/// (object/array/string/number/bool/null) is exactly this language's own
/// literal grammar, and `eval` already turns any `Expr` into a `Value`.
/// Bare `eval` is private and needs an `Environment` because a general
/// expression *can* reference variables; a literal never does, so a
/// throwaway empty one is always enough here — an identifier in `expr`
/// would still correctly fail as "undefined variable", exactly as it should
/// for something that's supposed to be pure JSON.
pub fn eval_literal(expr: &Expr) -> Result<Value, String> {
    eval(expr, &Environment::default())
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
        "Timestamp" => {
            // Whole seconds since the Unix epoch — the old language's
            // `Timestamp` did exactly this, and human-readable formatting
            // belongs in a module, not core (see docs/todo/community-modules.md).
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as f64;
            Ok(core_result("TimestampResult", secs))
        }
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
        // With exactly one array operand, the other is one *element* rather
        // than a sequence — appending or prepending it. Both arms sit after
        // the `Array + Array` case above, so two arrays still concatenate;
        // that is what keeps `[1] + [2]` = `[1, 2]` rather than `[1, [2]]`.
        (BinOp::Add, Array(a), item) => {
            let mut items = (**a).clone();
            items.push(item.clone());
            Some(Array(Rc::new(items)))
        }
        (BinOp::Add, item, Array(b)) => {
            let mut items = Vec::with_capacity(b.len() + 1);
            items.push(item.clone());
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

use std::collections::{HashMap, HashSet};
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
    /// Handlers the program defines itself (`Stmt::HandlerDef`), by class
    /// name. Flat and program-wide rather than scoped, because a definition
    /// is top-level only — and collected in one pass *before* execution
    /// starts, so a handler can emit to one defined further down the file,
    /// which is what lets a handler emit to one defined further down.
    handlers: HashMap<String, Rc<HandlerBody>>,
    /// One drain per linked module that can speak first — each returns
    /// whatever that module has queued since it was last called. A closure
    /// rather than the queue itself for the same reason `ModuleDispatch` is
    /// one: it keeps this field free of any `native-modules`-only type, so
    /// `crates/code-wasm` compiles with the feature off.
    inbound: Vec<Rc<dyn Fn() -> Vec<Value>>>,
    /// Handlers currently on the call stack. `handlers::check_cycles` already
    /// rejects every cycle it can see, but dispatch is by the particle's
    /// runtime `_class`, so a particle held in a variable names a handler no
    /// static pass could have resolved. This catches those.
    active: HashSet<String>,
}

/// A registered handler: the fields to seed its scope with, and the body to
/// run. `Rc` so invoking one doesn't clone the statements.
#[derive(Debug)]
struct HandlerBody {
    fields: Vec<String>,
    body: Vec<Stmt>,
}

/// Derived `Debug` doesn't work once a field holds a `dyn Fn` (no `Debug`
/// impl for closures) — written by hand instead, showing `modules`' keys
/// only, not the dispatchers themselves.
impl fmt::Debug for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Environment")
            .field("scopes", &self.scopes)
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
            modules: HashMap::new(),
            available_modules: HashMap::new(),
            handlers: HashMap::new(),
            inbound: Vec::new(),
            active: HashSet::new(),
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
    /// Registers a linked module's inbound queue, so `drain_inbound` will
    /// pick up whatever it pushes. Separate from `link_module` because most
    /// modules never speak first — `code_module_set_inbound` is optional.
    pub fn link_inbound(&mut self, drain: Rc<dyn Fn() -> Vec<Value>>) {
        self.inbound.push(drain);
    }

    pub fn provide_module(&mut self, name: &str, vars: Value, dispatch: ModuleDispatch) {
        self.available_modules
            .insert(name.to_string(), (vars, dispatch));
    }

    /// `let name = value` — always a new binding in the current scope,
    /// even if `name` already exists further out (shadowing) or even in
    /// this exact scope (re-`let`, just rebinds).
    fn declare(&mut self, name: String, value: Value) {
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
}

/// Hands every particle a linked module has queued to the program's own
/// handlers, and keeps going until nothing new appears — a handler that
/// causes further pushes has them picked up by the next round.
///
/// Runs after each *top-level* statement, which is what makes it
/// deterministic and testable: a fixture can assert on exactly what has been
/// handled by a given line. A continuously-draining keep-alive loop, which
/// is what an interactive daemon needs, is the next phase — see
/// `docs/todo/inbound-emissions-from-native-modules.md`.
///
/// Queued particles go to the *program's* handlers (`EmitTarget::This`),
/// never back to the module that pushed them: the module supplies events,
/// the program decides what they mean. A class nothing handles is an error,
/// the same answer `emit ... to this` gives.
fn drain_inbound(env: &mut Environment) -> Result<(), String> {
    loop {
        let drains = env.inbound.clone();
        let mut queued = Vec::new();
        for drain in &drains {
            queued.extend(drain());
        }
        if queued.is_empty() {
            return Ok(());
        }
        for particle in queued {
            // A pushed class the program has no handler for is *dropped*,
            // not an error — decided 2026-08-28, when `net` gained
            // `Log`/`Exception`. A module speaks first on its own
            // initiative; a diagnostic nobody asked to hear is not a
            // mistake by the program, and making it fatal would mean every
            // program that links a module which *might* report something
            // has to handle it. `emit ... to this` is unchanged: that is
            // the program addressing itself, and a class it does not handle
            // there is still a bug. The cost of this, taken knowingly: a
            // module pushing a mistyped class now goes unnoticed.
            if env.handlers.contains_key(class_of(&particle)) {
                dispatch_handler(&particle, env)?;
            }
        }
    }
}

/// Collects every `Stmt::HandlerDef` into `env.handlers` before a single
/// statement runs. Hoisting is what lets a handler emit to one defined later
/// in the file — without it, which handlers a body could reach would depend
/// on definition order.
///
/// Hoisting does *not* permit cycles: `handlers::check_cycles` rejects those
/// before a statement runs.
///
/// Descends into `Stmt::Import` bodies (a linked module's top level is a top
/// level too, so its handlers join the same program-wide table) and nothing
/// else: the parser already rejects a definition inside an `if`, a block, or
/// a loop.
fn register_handlers(stmts: &[Stmt], env: &mut Environment) -> Result<(), String> {
    for stmt in stmts {
        match stmt {
            Stmt::HandlerDef {
                class_name,
                fields,
                body,
            } => {
                if env.handlers.contains_key(class_name) {
                    return Err(format!(
                        "duplicate handler for '{class_name}': only one handler per class"
                    ));
                }
                env.handlers.insert(
                    class_name.clone(),
                    Rc::new(HandlerBody {
                        fields: fields.clone(),
                        body: body.clone(),
                    }),
                );
            }
            Stmt::Import { body, .. } => register_handlers(body, env)?,
            _ => {}
        }
    }
    Ok(())
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
    register_handlers(&program.statements, &mut env)?;
    crate::handlers::check_cycles(program)?;
    // The same pre-run check `code build` has always run (`verify.rs`, which
    // is where it moved out of `codegen.rs` on 2026-08-28 so both backends
    // could share it). Interpreting used to find an undefined name only on
    // reaching it, which was untidy but harmless while that ended the program
    // either way. Phase 4 made it matter: an error inside a handler body is
    // now an `Exception` value, so an undefined name there would leave the
    // interpreted program running to completion while `code build` refused it
    // outright — the two modes disagreeing about which programs fail, which is
    // the one thing they may not do.
    crate::verify::verify_defined(program)?;
    for (i, stmt) in program.statements.iter().enumerate() {
        // Can only ever be `Flow::Normal` out here: the parser rejects a
        // `break` that isn't inside a loop, so nothing can propagate one up
        // to the top level.
        //
        // The one place a runtime error learns where it came from. Every
        // error site below stays a plain `String` — the position is attached
        // once, here, from the statement being executed, exactly as
        // `parser::parse` does it for parse errors. A failure nested in an
        // `if` or `loop` body surfaces here too, and so reports the
        // *enclosing* top-level statement; see `Program::starts`.
        exec(stmt, &mut env)
            .and_then(|_| drain_inbound(&mut env))
            .map_err(|msg| locate(program, i, msg))?;
    }
    Ok(env)
}

/// Renders `msg` against the source `program` came from, pointing at the
/// top-level statement at index `i`. Returns it unchanged when the program
/// carries no origin (a hand-built one) or no offset for `i`.
fn locate(program: &Program, i: usize, msg: String) -> String {
    let Some(origin) = &program.origin else {
        return msg;
    };
    crate::span::render(
        &origin.source,
        &origin.file,
        program.starts.get(i).copied(),
        &msg,
    )
}

/// Whether a statement finished normally or hit a `break` that the innermost
/// enclosing `Stmt::Loop` still has to act on. Nested `if`/block bodies just
/// pass it straight through — a `break` inside an `if` inside a loop breaks
/// the loop, not the `if`.
#[derive(Debug, Clone, PartialEq)]
enum Flow {
    Normal,
    Break,
    /// `return <particle>` — unwinds to the enclosing handler body, which is
    /// the only thing that ever consumes it. Propagates outward through
    /// `if`/block/loop bodies like `Break`, but no loop absorbs it.
    Return(Value),
    /// Like `Break`, but the enclosing loop starts its next iteration
    /// instead of stopping. Propagates outward through `if`/block bodies
    /// exactly the same way.
    Continue,
}

/// A snapshotted `Stmt::Loop` container — an `Array` or an `Object`, indexed
/// uniformly as `len()` positions each yielding `(key, value)`. Shaped as
/// exactly this pair because it is what `runtime.c`'s `code_iter_len` /
/// `code_iter_at` / `code_iter_key` already expose, so the interpreter and
/// the compiled backend read as the same algorithm — see `Stmt::Loop`'s doc
/// comment for the law (`X[k] = v`) this exists to satisfy.
enum LoopIter {
    Array(Rc<Vec<Value>>),
    Object(Rc<Vec<(String, Value)>>),
}

impl LoopIter {
    fn len(&self) -> usize {
        match self {
            LoopIter::Array(items) => items.len(),
            LoopIter::Object(fields) => fields.len(),
        }
    }

    /// `i` is always in range — the only caller is the loop above, which
    /// only ever asks for `0..self.len()`.
    fn at(&self, i: usize) -> (Value, Value) {
        match self {
            LoopIter::Array(items) => (Value::Number(i as f64), items[i].clone()),
            LoopIter::Object(fields) => {
                let (key, value) = &fields[i];
                (Value::Str(Rc::from(key.as_str())), value.clone())
            }
        }
    }
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
                    // Taken before the module moves into the closure below;
                    // the queue outlives both (it was leaked at `open`).
                    let inbound = module.inbound_handle();
                    let dispatch: ModuleDispatch = Rc::new(move |v| module.dispatch(v));
                    env.link_module(alias, Value::Object(Rc::new(vars)), dispatch);
                    env.link_inbound(Rc::new(move || inbound.take()));
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
                "assert requires a boolean, found {}",
                a_type_name(&v)
            )),
        },
        Stmt::If { condition, body } => match eval(condition, env)? {
            Value::Bool(true) => exec_scoped_body(body, env),
            Value::Bool(false) => Ok(Flow::Normal),
            v => Err(format!("if requires a boolean, found {}", a_type_name(&v))),
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
                    // whatever binding the container came from without
                    // disturbing the iteration (and since no value is ever
                    // mutated in place, the snapshot can't go stale either
                    // way — see memory `new-code-memory-management`).
                    // Matched by reference, not by move: `Value` has a
                    // manual `Drop` (see value.rs), and Rust forbids moving a
                    // field out of such a type. `Rc::clone` is the O(1)
                    // equivalent anyway.
                    let evaluated = eval(&over.iterable, env)?;
                    let container = match &evaluated {
                        Value::Array(items) => LoopIter::Array(Rc::clone(items)),
                        Value::Object(fields) => LoopIter::Object(Rc::clone(fields)),
                        v => {
                            return Err(format!(
                                "loop requires an array or object, found {}",
                                a_type_name(v)
                            ))
                        }
                    };
                    for i in 0..container.len() {
                        let (key, value) = container.at(i);
                        env.push_scope();
                        if let Some(key_name) = &over.key {
                            env.declare(key_name.clone(), key);
                        }
                        env.declare(over.value.clone(), value);
                        let flow = exec_body(body, env);
                        env.pop_scope();
                        // `Continue` needs no action beyond ending this
                        // iteration, which returning from the body already
                        // did — only `Break` changes what happens next.
                        // `Return` belongs to the enclosing handler, so it
                        // passes straight through rather than being absorbed.
                        match flow? {
                            Flow::Break => break,
                            Flow::Return(value) => return Ok(Flow::Return(value)),
                            _ => {}
                        }
                    }
                }
                // `loop { }` — nothing bounds this but `break`. See
                // `Stmt::Loop`'s doc comment on the guarantee that gives up.
                None => loop {
                    env.push_scope();
                    let flow = exec_body(body, env);
                    env.pop_scope();
                    match flow? {
                        Flow::Break => break,
                        Flow::Return(value) => return Ok(Flow::Return(value)),
                        _ => {}
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
            // Asked once, here, before the target is even looked at: whether
            // something is a particle has nothing to do with where it was
            // being sent. Until 2026-08-28 each target asked for itself —
            // five sites across two backends, in two different wordings — and
            // the module path could not ask at all: a module reads `_class`,
            // finds none, and cannot tell "not a particle" from "a class I
            // don't handle", so it answered null to both. `emit 5 to math`
            // therefore did nothing while `emit 5 to this` failed.
            //
            // A non-particle `emit` is the *emitting* frame's own mistake, not
            // something a recipient did, so it fails here and the frame
            // returns an `Exception` — no target is dispatched to at all.
            // Must match codegen.rs's `gen_emit`.
            check_emittable(&value)?;
            let output = match target {
                EmitTarget::Core => dispatch_core(&value)?,
                EmitTarget::This => dispatch_handler(&value, env)?,
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
        // Already collected by `register_handlers` before execution began —
        // reaching the definition in statement order does nothing.
        Stmt::HandlerDef { .. } => Ok(Flow::Normal),
        Stmt::Return(expr) => Ok(Flow::Return(eval(expr, env)?)),
    }
}

/// `emit <particle> to this` — runs the handler registered for the
/// particle's own `_class`.
///
/// The body's enclosing scope is the **top level**, never the caller's: the
/// scope stack is temporarily cut down to its outermost frame for the
/// duration, so a handler invoked from inside a loop or another handler sees
/// exactly what one invoked from the top of the file would. Cutting rather
/// than copying keeps top-level *writes* live — a handler reassigning a
/// top-level binding is the point of `handler_outer_scope.code`.
/// A particle's `_class`, or `""` for anything that isn't a classed object
/// — which `dispatch_handler` would reject anyway, and no handler is named
/// `""`, so an unclassed *pushed* value drops like any other unhandled one.
fn class_of(particle: &Value) -> &str {
    match particle {
        Value::Object(fields) => match fields.iter().find(|(k, _)| k == "_class") {
            Some((_, Value::Str(class))) => class,
            _ => "",
        },
        _ => "",
    }
}

/// Whether `value` can be emitted at all: emitting is dispatch by `_class`,
/// so a value that carries none is not a particle and there is nothing to
/// dispatch on. Deliberately *not* the same question as "does anyone handle
/// this class" — that one answers null, because sending a particle is not a
/// demand and whether to act on it is the recipient's business.
///
/// Worded to parallel `code_check_particle`'s message for the other half of
/// the same rule, a handler's `return`. Must match `runtime.c`'s
/// `code_check_emittable` exactly — `tests/message_parity.rs` compares them.
fn check_emittable(value: &Value) -> Result<(), String> {
    if let Value::Object(fields) = value {
        if fields.iter().any(|(k, _)| k == "_class") {
            return Ok(());
        }
    }
    Err(format!(
        "emit requires a particle — an object with a '_class' field — found {}",
        a_type_name(value)
    ))
}

/// `Exception { source, message, innerException }` — a frame's answer when it
/// could not finish. Must stay byte-identical to `runtime.c`'s
/// `code_make_exception`, field order included: it is an ordinary value the
/// program can read, so the two backends building it differently would be a
/// visible divergence rather than a cosmetic one.
///
/// `source` is `"core"` because that is the language's own name for what runs
/// a program's own statements; a module names itself instead. `inner` is null
/// here — nothing wraps a language-level failure yet.
fn exception(message: String) -> Value {
    Value::Object(Rc::new(vec![
        ("_class".to_string(), Value::Str("Exception".into())),
        ("source".to_string(), Value::Str("core".into())),
        ("message".to_string(), Value::Str(message.into())),
        ("innerException".to_string(), Value::Null),
    ]))
}

fn dispatch_handler(particle: &Value, env: &mut Environment) -> Result<Value, String> {
    // `check_emittable` ran at the emit site, so a `_class` is here. A
    // *non-Str* one is not a particle either and takes the same path as an
    // unknown class: null. There is no third answer to give — this function
    // is only ever reached through `emit`.
    let class = match particle {
        Value::Object(fields) => fields.iter().find(|(k, _)| k == "_class").map(|(_, v)| v),
        _ => None,
    };
    let Some(Value::Str(class)) = class else {
        return Ok(Value::Null);
    };
    // A class nothing handles answers null rather than ending the program:
    // sending a particle is not a demand, and whether to act on one is the
    // recipient's business (decided 2026-08-28, see
    // docs/todo/errors-as-particles.md). The same answer `to core` and a
    // native module give.
    let Some(handler) = env.handlers.get(class.as_ref()).cloned() else {
        return Ok(Value::Null);
    };
    // Re-entry is the *emit's* failure, so it is this dispatch's answer rather
    // than an error thrown into the caller's body: the frame that tried to
    // re-enter gets an Exception back and decides what to do, and the handler
    // already running is untouched. `codegen.rs` reaches the same shape from
    // the other side — the re-entered function writes the Exception into its
    // own `out` and returns without clearing the guard.
    //
    // Decided 2026-08-28 (phase 4): a cycle answers rather than aborts, like
    // every other runtime failure. `handlers::check_cycles` still refuses the
    // statically visible ones before either backend runs at all; this is only
    // for the cycles that go through a variable.
    if !env.active.insert(class.to_string()) {
        return Ok(exception(format!(
            "handler '{class}' is already running — a handler cannot re-enter one \
             that is already on the call stack"
        )));
    }

    // Everything but the top-level frame steps aside for the call.
    let saved: Vec<HashMap<String, Value>> = env.scopes.drain(1..).collect();

    // A listed field the particle doesn't carry is null — the same answer
    // `.field` gives for an absent member.
    let mut seeded = HashMap::new();
    for name in &handler.fields {
        let supplied = match particle {
            Value::Object(fields) => fields
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone()),
            _ => None,
        };
        seeded.insert(name.clone(), supplied.unwrap_or(Value::Null));
    }

    env.scopes.push(seeded);
    let flow = exec_body(&handler.body, env);
    env.pop_scope();
    let result = match flow {
        // No `return` at all: the result is null. Handlers that exist for
        // their effect rather than their answer are ordinary.
        Ok(Flow::Normal) => Ok(Value::Null),
        Ok(Flow::Return(value)) => match &value {
            Value::Object(fields) if fields.iter().any(|(k, _)| k == "_class") => Ok(value),
            other => Ok(exception(format!(
                "a handler must return a particle — an object with a '_class' field — found {}",
                a_type_name(other)
            ))),
        },
        // The parser rejects `break`/`continue` outside a loop, so neither
        // can escape a body to here.
        Ok(_) => Ok(Value::Null),
        // A runtime error inside the body ends *this frame*, not the program:
        // the handler's answer becomes an Exception and the caller carries on
        // (shipped 2026-08-28, phase 4 of docs/todo/errors-as-particles.md).
        // `codegen.rs`'s `check_failed` does the same thing by branching to
        // the frame's exit with `code_take_failure` — the two backends have
        // to agree here, since the result is a value the program can see.
        //
        // Only errors raised *by the body* convert. The two above this — a
        // non-particle `emit` operand and a re-entered handler — happen
        // before the body runs and belong to the caller, so they still
        // propagate.
        Err(e) => Ok(exception(e)),
    };

    env.scopes.extend(saved);
    env.active.remove(class.as_ref());
    result
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
        Expr::Interpolated(parts) => {
            let mut out = String::new();
            for part in parts {
                match &eval(part, env)? {
                    // Bare, not quoted — see `Expr::Interpolated`'s doc.
                    Value::Str(s) => out.push_str(s),
                    other => out.push_str(&other.to_string()),
                }
            }
            Ok(Value::Str(Rc::from(out.as_str())))
        }
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
        // Two different situations, deliberately answered differently
        // (revised 2026-08-23, replacing a blanket permissive-null rule):
        // the *wrong kind* of operand is a mistake and errors, while a
        // member that merely isn't there is null. `"abc"[0]` silently
        // yielding null hid real bugs; `obj.absent` being null is load-
        // bearing, since it is how an un-exported module name reads.
        Expr::Field(obj, field) => {
            let v = eval(obj, env)?;
            match &v {
                // A *missing* field is still null — that is what makes a
                // module's un-exported name read as null through its alias
                // (`link_default_private.code`). Only the wrong *kind* of
                // operand is an error.
                Value::Object(fields) => Ok(fields
                    .iter()
                    .find(|(k, _)| k == field)
                    .map(|(_, v)| v.clone())
                    .unwrap_or(Value::Null)),
                v => Err(format!(
                    "cannot read field '{field}' of {} — '.' requires an object",
                    a_type_name(v)
                )),
            }
        }
        Expr::Index(arr, index) => {
            let v = eval(arr, env)?;
            let i = eval(index, env)?;
            match &v {
                // An out-of-range or non-integer index stays null, for the
                // same reason a missing field does: the operand kind is
                // right, the lookup simply found nothing.
                Value::Array(items) => Ok(match &i {
                    Value::Number(n) if n.fract() == 0.0 && *n >= 0.0 => {
                        items.get(*n as usize).cloned().unwrap_or(Value::Null)
                    }
                    _ => Value::Null,
                }),
                // `obj[key]` — a *computed* field read, the thing `.` can
                // never offer since its name is always a bare identifier.
                // Same absent-is-null rule as `Field`; a non-`Str` key is
                // also just null, not an error, matching `Array`'s
                // non-`Number` case above.
                Value::Object(fields) => Ok(match &i {
                    Value::Str(key) => fields
                        .iter()
                        .find(|(k, _)| k.as_str() == key.as_ref())
                        .map(|(_, v)| v.clone())
                        .unwrap_or(Value::Null),
                    _ => Value::Null,
                }),
                v => Err(format!(
                    "cannot index {} — '[]' requires an array or object",
                    a_type_name(v)
                )),
            }
        }
        Expr::Unary(op, e) => {
            let v = eval(e, env)?;
            match (op, v) {
                (UnOp::Neg, Value::Number(n)) => Ok(Value::Number(-n)),
                (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
                (UnOp::Neg, v) => Err(format!("cannot negate {}", a_type_name(&v))),
                (UnOp::Not, v) => Err(format!(
                    "'not' requires a boolean, found {}",
                    a_type_name(&v)
                )),
            }
        }
        // `expr is ClassName` — a type test, not a lookup: true exactly
        // when `expr` is a particle of that class, false otherwise (an
        // object without a `"_class"` field, a wrong class, or any
        // non-object value). Never an error — see `Expr::Is`'s doc comment.
        Expr::Is(e, class) => {
            let v = eval(e, env)?;
            let is_particle_of_class = matches!(&v, Value::Object(fields)
                if fields.iter().any(|(k, val)| k == "_class"
                    && matches!(val, Value::Str(s) if **s == *class)));
            Ok(Value::Bool(is_particle_of_class))
        }
        // `and`/`or` short-circuit: the right side is only evaluated (and
        // only needs to be a bool) when the left side didn't already
        // determine the result.
        Expr::Binary(lhs, BinOp::And, rhs) => match eval(lhs, env)? {
            Value::Bool(false) => Ok(Value::Bool(false)),
            Value::Bool(true) => require_bool(eval(rhs, env)?, "and"),
            v => Err(format!(
                "'and' requires booleans, found {}",
                a_type_name(&v)
            )),
        },
        Expr::Binary(lhs, BinOp::Or, rhs) => match eval(lhs, env)? {
            Value::Bool(true) => Ok(Value::Bool(true)),
            Value::Bool(false) => require_bool(eval(rhs, env)?, "or"),
            v => Err(format!("'or' requires booleans, found {}", a_type_name(&v))),
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
    // Same as `dispatch_handler`: `check_emittable` ran at the emit site, so
    // anything that gets here without a Str `_class` is simply a class core
    // does not know, and core answers null like any other recipient.
    let Value::Object(fields) = particle else {
        return Ok(Value::Null);
    };
    let class = match fields.iter().find(|(k, _)| k == "_class") {
        Some((_, Value::Str(class))) => class,
        _ => return Ok(Value::Null),
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
        // A field the particle does not carry is null — the same answer
        // `.field` gives — so an absent `value` is not a separate case to
        // report. Emitting a particle is not a form to be validated before
        // the handler may run: `Length { }` means `Length { "value": null }`,
        // and null has no length. Must match runtime.c's `Length` arm.
        "Length" => match fields
            .iter()
            .find(|(k, _)| k == "value")
            .map_or(&Value::Null, |(_, v)| v)
        {
            Value::Array(items) => Ok(core_result("LengthResult", items.len() as f64)),
            // Characters, not bytes — `len()` reported 6 for "héllo". Must
            // match runtime.c's continuation-byte count exactly.
            Value::Str(s) => Ok(core_result("LengthResult", s.chars().count() as f64)),
            // Core answers rather than unwinding its caller, the same as a
            // module and the same as a handler written in the language:
            // `core` is a recipient like any other, so `emit Length { } to
            // core get r` binds `r` instead of ending the frame that emitted
            // (2026-08-28). Must match runtime.c's `Length` arm.
            //
            // Only failures from *here* answer this way. A malformed emit
            // (`emit 5 to core`) is the emitting frame's own mistake and
            // still fails there, exactly as `emit 5 to this` does — which is
            // why the two checks above this still return `Err`.
            v => Ok(exception(format!(
                "Length requires an array or string 'value', found {}",
                a_type_name(v)
            ))),
        },
        // Not a core class. Null, for the same reason `dispatch_handler`
        // gives one — core is a recipient like any other.
        _ => Ok(Value::Null),
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
            "'{op}' requires booleans, found {}",
            a_type_name(&v)
        )),
    }
}

/// `type_name` with the right indefinite article — "a number", "an array".
/// Every message below reads "found {a_type_name(v)}" rather than hardcoding
/// "a {type_name(v)}", which used to produce "a array" / "a object".
fn a_type_name(v: &Value) -> String {
    let name = type_name(v);
    let article = match name.chars().next() {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    };
    format!("{article} {name}")
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
        // Two objects merge, the way two arrays concatenate. Arrays have no
        // duplicate keys to reconcile and objects do, so this arm has the
        // one extra rule: a field both sides name takes the *right* value
        // in the *left* position, which is what makes `a + b` mean "a, with
        // b's fields applied" rather than "a, then b appended". Order is
        // load-bearing — `PartialEq` compares objects pairwise in order.
        (BinOp::Add, Object(a), Object(b)) => {
            let mut fields: Vec<(String, Value)> = Vec::with_capacity(a.len() + b.len());
            for (key, value) in a.iter() {
                let overridden = b.iter().find(|(k, _)| k == key).map(|(_, v)| v);
                fields.push((key.clone(), overridden.unwrap_or(value).clone()));
            }
            for (key, value) in b.iter() {
                if !a.iter().any(|(k, _)| k == key) {
                    fields.push((key.clone(), value.clone()));
                }
            }
            Some(Object(Rc::new(fields)))
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
    // `op.symbol()`, not `{op:?}`: this used to read "cannot apply Add to
    // number and string" while the compiled program said "cannot apply '+' to
    // these values" for the very same program. Both now say the same thing,
    // and `tests/message_parity.rs` keeps it that way.
    result.ok_or_else(|| {
        format!(
            "cannot apply '{}' to {} and {}",
            op.symbol(),
            a_type_name(&l),
            a_type_name(&r)
        )
    })
}

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::ast::{
    BinOp, EmitTarget, Expr, FieldKey, IsTest, NativeFormat, Program, Stmt, UnOp, ValueKind,
};
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

/// The field an address value carries. Underscore-prefixed for the same
/// reason `_class` is: it names something the runtime put there, not
/// something the program wrote, and the prefix is what keeps the two from
/// ever being confused for each other.
pub const MODULE_FIELD: &str = "_module";

/// One module opened by a `link` inside a handler — see
/// `ast::Stmt::LinkRuntime`.
///
/// Unloading is not a method here: it happens when the row holding this is
/// dropped, because that is when the last `Rc` to the underlying module
/// goes. `release` is the *other* half, and has to run first — the module's
/// own "you may let go now" point, while its code is still mapped.
struct RuntimeModule {
    dispatch: ModuleDispatch,
    release: Rc<dyn Fn()>,
    /// Hands it a turn to deliver whatever its own modules pushed. A
    /// library has queues but no loop of its own — see `NativeModule::drain`.
    drain: Rc<dyn Fn()>,
    /// Whether anything it holds is still working — what decides if it can
    /// be unloaded at all.
    serving: Rc<dyn Fn() -> bool>,
    /// Which guest row this program keeps for it, or `None` for a module
    /// that cannot be hosted (one built before `code_abi.h` item 10). A row,
    /// not a pointer — see `native.rs`'s hosting tables.
    #[cfg(feature = "native-modules")]
    guest: Option<usize>,
    /// Which `Environment::inbound` row listens to it, for a module that
    /// speaks first — `None` for the rest, which is most of them.
    inbound: Option<usize>,
}

/// The value a `link` inside a handler binds: an ordinary object, so
/// nothing about the language's six value kinds changes.
fn module_address(row: usize) -> Value {
    Value::Object(Rc::new(vec![(
        MODULE_FIELD.to_string(),
        Value::Number(row as f64),
    )]))
}

/// The row an address names, or an error naming what was passed instead.
/// Deliberately strict about the shape: an address is something the runtime
/// minted, so anything else is a program mistake worth reporting precisely
/// rather than a lookup that quietly finds nothing.
fn address_row(value: &Value) -> Result<usize, String> {
    let Value::Object(fields) = value else {
        return Err(format!(
            "expected a module address (from a 'link' inside a handler), found {}",
            a_type_name(value)
        ));
    };
    match fields.iter().find(|(k, _)| k == MODULE_FIELD) {
        Some((_, Value::Number(n))) if *n >= 0.0 && n.fract() == 0.0 => Ok(*n as usize),
        _ => Err(
            "expected a module address (from a 'link' inside a handler), found an \
             ordinary object"
                .to_string(),
        ),
    }
}

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
    /// Every linked file's top level, kept for as long as the program runs.
    ///
    /// A handler belongs to the file it was written in, and that file's names
    /// are its whole world — so the world has to outlive the `link` that ran
    /// it. The statements are over; the handlers are not.
    ///
    /// The file *currently running* is the exception: its scope is moved out
    /// of here and sits at the bottom of `scopes`, leaving an empty entry
    /// behind. `current_file` says which one that is. Moved rather than
    /// shared because a handler writing to `count` has to be writing to the
    /// same map its file's top level declared.
    file_scopes: Vec<HashMap<String, Value>>,
    /// Which file's statements are running — the index whose `file_scopes`
    /// entry is currently empty because `scopes[0]` holds it.
    current_file: usize,
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
    /// Modules opened by a `link` that ran *inside a handler*
    /// (`Stmt::LinkRuntime`), indexed by the number an address value
    /// carries. Separate from `modules` because these have no alias to be
    /// found by — the program holds them as values and may pass them
    /// around, so the table is the only thing that outlives the binding.
    ///
    /// `unlink` leaves a `None` behind rather than removing the row: an
    /// address the program is still holding then names an empty row and
    /// answers with an `Exception`, instead of silently naming whichever
    /// module was opened next.
    runtime_modules: Vec<Option<RuntimeModule>>,
    /// Handlers the program defines itself (`Stmt::HandlerDef`), by class
    /// name. Flat and program-wide rather than scoped, because a definition
    /// is top-level only — and collected in one pass *before* execution
    /// starts, so a handler can emit to one defined further down the file,
    /// which is what lets a handler emit to one defined further down.
    handlers: HashMap<String, Rc<HandlerBody>>,
    /// One table per level of the module graph, filled by
    /// `register_handlers` before anything runs: index `i` holds the
    /// handlers defined at depth `i` (top level is `0`). Sibling modules
    /// never collide in one table because the duplicate-handler rule is
    /// program-wide, so depth alone identifies a table. `emit … to base`
    /// looks up `module_depth - 1` — the *direct* parent, never the whole
    /// program. Empty at the top level, where `to base` is illegal anyway
    /// (`verify.rs` refuses it before either backend starts).
    handler_tables: Vec<HashMap<String, Rc<HandlerBody>>>,
    /// How many `Stmt::Import` bodies are currently open while executing —
    /// the depth of the statement being run. Incremented and decremented
    /// by the `Import` arm of `exec`; a plain counter is enough because a
    /// handler body can never contain a `link` (top-level only) and the
    /// inbound drain only runs between top-level statements.
    module_depth: usize,
    /// One drain per linked module that can speak first — each returns
    /// whatever that module has queued since it was last called. A closure
    /// rather than the queue itself for the same reason `ModuleDispatch` is
    /// one: it keeps this field free of any `native-modules`-only type, so
    /// `crates/code-wasm` compiles with the feature off.
    inbound: Vec<InboundSource>,
    /// What a linked module's queue signals after pushing, and what
    /// `keep_alive` sleeps on. One per environment — see [`Wakeup`].
    wakeup: Arc<Wakeup>,
    /// Handlers currently on the call stack. `handlers::check_cycles` already
    /// rejects every cycle it can see, but dispatch is by the particle's
    /// runtime `_class`, so a particle held in a variable names a handler no
    /// static pass could have resolved. This catches those.
    active: HashSet<String>,
}

/// A registered handler: the fields to seed its scope with, the body to
/// run, and the level of the module graph the definition sits at — restored
/// as `module_depth` for the duration of the call, so a `to base` inside
/// the body means *this* handler's parent, whoever invoked it.
#[derive(Debug)]
struct HandlerBody {
    fields: Vec<String>,
    body: Vec<Stmt>,
    defining_depth: usize,
    /// The file this handler was written in — `Environment::file_scopes`'
    /// index, and the only world the body can see. A handler in a linked
    /// file cannot reach the names of the program that linked it, exported
    /// or not: the link has a direction, and the way back up is `emit ... to
    /// base`, which reaches handlers rather than names.
    file: usize,
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
            file_scopes: vec![HashMap::new()],
            current_file: 0,
            modules: HashMap::new(),
            available_modules: HashMap::new(),
            runtime_modules: Vec::new(),
            handlers: HashMap::new(),
            handler_tables: Vec::new(),
            module_depth: 0,
            inbound: Vec::new(),
            active: HashSet::new(),
            wakeup: Arc::new(Wakeup::default()),
        }
    }
}

impl Environment {
    /// This environment's own wakeup, for a host wiring a module's queue to
    /// it. Cloned rather than borrowed: the queue outlives the call and is
    /// signalled from the module's thread.
    pub fn wakeup(&self) -> Arc<Wakeup> {
        Arc::clone(&self.wakeup)
    }

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

    #[cfg(feature = "native-modules")]
    /// Files a runtime-linked module and hands back the address value
    /// naming it — see `ast::Stmt::LinkRuntime`.
    ///
    /// Rows are appended and never reused, even after `unlink` empties one.
    /// Reuse would make a stale address a *live* one pointing at an
    /// unrelated module, which is the one failure this table exists to
    /// prevent; an ever-growing `Vec` of `None`s is the cheaper problem.
    fn open_module(&mut self, module: RuntimeModule) -> Value {
        self.runtime_modules.push(Some(module));
        module_address(self.runtime_modules.len() - 1)
    }

    /// Releases and drops a module. Dropping the row is what unloads it:
    /// the module's `Drop` runs when the last `Rc` inside goes, and the
    /// release point has to have run before that, while its code is still
    /// mapped.
    fn release_module(&mut self, row: usize) {
        let Some(slot) = self.runtime_modules.get_mut(row) else {
            return;
        };
        let Some(module) = slot.take() else {
            return;
        };
        // Empty this guest's rows first: its stand-ins must stop answering
        // before it is told to let go of everything, not after.
        #[cfg(feature = "native-modules")]
        if let Some(guest) = module.guest {
            crate::native::close_hosted_guest(guest);
        }
        // And stop listening to it, for the same reason and one more: the
        // row holds the `Rc`s that keep the module loaded, so a row left
        // behind is a module that never unloads.
        if let Some(row) = module.inbound {
            self.drop_inbound(row);
        }
        (module.release)();
    }

    /// Closes whatever is still linked when the program ends — the same
    /// rule `runtime.c`'s `code_runtime_unlink_all` follows, and for the
    /// same reason: a guest still holding its world at exit is a guest whose
    /// release point never ran, which is exactly what `unlink` exists to
    /// guarantee.
    fn unlink_all(&mut self) {
        for row in 0..self.runtime_modules.len() {
            while self.runtime_modules[row].is_some() {
                self.release_module(row);
            }
        }
    }

    /// The dispatcher an address names, or an error saying why not.
    fn module_at(&self, address: &Value) -> Result<ModuleDispatch, String> {
        let row = address_row(address)?;
        match self.runtime_modules.get(row) {
            Some(Some(module)) => Ok(Rc::clone(&module.dispatch)),
            // Both readings of "nothing here" are the same mistake from the
            // program's side — an address that named something once and
            // does not now — so they read the same.
            Some(None) | None => Err("this module has been unlinked".to_string()),
        }
    }

    /// Releases and drops a runtime-linked module. Dropping the row is
    /// what unloads it: the module's `Drop` runs once the last `Rc` inside
    /// the row goes, and the release point has to have run before that,
    /// while its code is still mapped.
    fn close_module(&mut self, address: &Value) -> Result<(), String> {
        let row = address_row(address)?;
        match self.runtime_modules.get(row) {
            Some(Some(module)) => {
                // Refused while anything it holds is still working —
                // unmapping code a thread is running in is a crash, not a
                // risk. See `runtime.c`'s `code_runtime_unlink`, which this
                // must match: a failure rather than a silent skip, so a host
                // can say the application is still running instead of
                // marking something stopped that is still answering.
                if (module.serving)() {
                    return Err("this module is still working — stop what it holds before \
                         unlinking it"
                        .to_string());
                }
                self.release_module(row);
                Ok(())
            }
            Some(None) | None => Err("this module has already been unlinked".to_string()),
        }
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
    pub fn link_inbound(
        &mut self,
        drain: Rc<dyn Fn() -> Vec<Value>>,
        reply: InboundReply,
        serving: Rc<dyn Fn() -> bool>,
    ) -> usize {
        self.inbound.push(InboundSource {
            drain,
            reply,
            serving,
        });
        self.inbound.len() - 1
    }

    /// Stops listening to one source, without disturbing the rows around it.
    ///
    /// A module linked while the program runs can also be unlinked while
    /// it runs, and then this program must stop draining it — and, just as
    /// importantly, stop holding the `Rc`s inside its row, which are what
    /// keep the module loaded. Overwritten rather than removed because every
    /// other row's index is an identity that outlives it.
    fn drop_inbound(&mut self, row: usize) {
        if let Some(slot) = self.inbound.get_mut(row) {
            *slot = InboundSource {
                drain: Rc::new(Vec::new),
                reply: Rc::new(|_, _| {}),
                serving: Rc::new(|| false),
            };
        }
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
/// A loop iteration is a statement boundary too.
///
/// Queued particles have been handed over between *top-level statements*
/// since inbound emissions shipped, which was enough while a program was a
/// straight line. It is not enough for a program that stays up: everything a
/// module pushed inside `loop { … }` sat in the queue until the loop ended,
/// so the one shape an event loop has to take was the one shape that did not
/// work (measured 2026-08-29 — `seen` came out `[99, 0, 1]` instead of
/// `[0, 1, 99]`).
///
/// Only outside a handler, which `env.active` already answers: it holds the
/// handlers currently on the call stack, so empty means top level. A drain
/// inside a handler's own loop would dispatch a particle into a handler while
/// one is running, and re-entry is exactly what the language forbids —
/// the loop would quietly fill with `Exception`s. `codegen.rs`'s `gen_loop`
/// makes the same test with `handler_frame`.
fn drain_between_iterations(env: &mut Environment) -> Result<(), String> {
    if env.active.is_empty() {
        drain_inbound(env)?;
    }
    Ok(())
}

fn drain_inbound(env: &mut Environment) -> Result<usize, String> {
    let mut handled = 0usize;
    loop {
        // Guests first, and here rather than anywhere of their own: a
        // library has queues but no loop to empty them, so this program's
        // drain is where they get their turn. An idle program never reaches
        // this, because nothing woke it.
        #[cfg(feature = "native-modules")]
        {
            let drains: Vec<Rc<dyn Fn()>> = env
                .runtime_modules
                .iter()
                .filter_map(|slot| slot.as_ref().map(|o| Rc::clone(&o.drain)))
                .collect();
            for drain in drains {
                drain();
            }
        }
        let sources = env.inbound.clone();
        // Kept per source rather than pooled: the answer has to go back to
        // the module that asked, and once the particles are in one list
        // there is nothing left to say which module that was.
        let mut queued: Vec<(usize, Value)> = Vec::new();
        for (i, source) in sources.iter().enumerate() {
            queued.extend((source.drain)().into_iter().map(|v| (i, v)));
        }
        if queued.is_empty() {
            return Ok(handled);
        }
        handled += queued.len();
        for (source, particle) in queued {
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
            // Null when nothing handled it, which the module is told as
            // plainly as it is told an answer: "nobody replied" is an answer
            // an HTTP server has to turn into a status.
            let answer = if env.handlers.contains_key(class_of(&particle)) {
                dispatch_handler(&particle, env)?
            } else {
                Value::Null
            };
            (sources[source].reply)(&particle, &answer);
        }
    }
}

/// Whether *any* linked module still expects to speak — the condition that
/// keeps the program up after its last statement. See [`keep_alive`].
fn any_module_serving(env: &Environment) -> bool {
    env.inbound.iter().any(|source| (source.serving)())
}

/// Raised by every push, waited on by [`keep_alive`].
///
/// **One signal for every module**, not one per queue: the program waits for
/// *something* to arrive, not for a particular module to speak, and a condvar
/// per queue would mean choosing which one to sleep on. `runtime.c` keeps a
/// single global for exactly the same reason.
///
/// A **count**, not a bare notification, because the two sides race by
/// design: a push can land between the drain that emptied the queues and the
/// wait that follows it. A signal sent in that window would be sent to
/// nobody, and the particle would sit there until the next re-check. A count
/// survives the gap — the waiter sees it is already non-zero and returns
/// without sleeping at all.
///
/// **One per `Environment`, not one per process.** A host that runs several
/// programs at once — a runtime holding an app each in its own environment —
/// gives every one of them its own. Shared, a push meant for one program
/// would wake all of them, and whichever woke first would consume the count
/// and leave the rest to sleep until the next re-check: a particle delivered
/// a second late for no reason its sender could see.
#[derive(Default)]
pub struct Wakeup {
    pending: Mutex<u64>,
    arrived: Condvar,
}

impl Wakeup {
    /// Called by a module's queue after it has pushed.
    pub fn signal(&self) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        *pending += 1;
        self.arrived.notify_all();
    }

    /// Sleep until something is pushed, or until it is time to re-ask who is
    /// still serving. Consumes the count whole: the drain that follows hands
    /// over everything queued in one pass, so one wait answers every push
    /// that led to it.
    fn wait(&self, timeout: Duration) {
        let pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let (mut pending, _) = self
            .arrived
            .wait_timeout_while(pending, timeout, |pending| *pending == 0)
            .unwrap_or_else(|e| e.into_inner());
        *pending = 0;
    }
}

/// How long a wait sleeps before re-asking whether anything is still serving.
///
/// **Not a poll interval** — delivery is exact, since every push signals. This
/// only bounds how long it takes to notice a module that *stopped* serving
/// without pushing anything on its way out (an `http_server` told to `Stop`).
/// One wakeup a second, and only while the program is otherwise idle.
const SERVING_RECHECK: Duration = Duration::from_secs(1);

/// Keeps the program up after its last statement, for as long as any linked
/// module is still expecting to speak.
///
/// This is the whole reason an app does not write a keep-alive loop of its
/// own. It is the same rule a JVM follows for non-daemon threads: `Listen`
/// starts a thread, that thread holds the program open, and the program ends
/// on its own once nothing holds it any more.
///
/// It cannot be a plain join. A pushed particle is dispatched to the
/// program's *own* handlers, which run here, on this thread, between
/// statements — a thread blocked in `join` is not between statements, and
/// every request would time out unanswered one frame below the handler that
/// should have answered it. So this parks, wakes on a push, drains, and parks
/// again: a join with a dispatch pump in it. `codegen.rs` emits the same loop
/// around `code_host_wait`.
pub fn keep_alive(env: &mut Environment) -> Result<(), String> {
    while any_module_serving(env) {
        Arc::clone(&env.wakeup).wait(SERVING_RECHECK);
        drain_inbound(env)?;
    }
    Ok(())
}

/// Hand one particle to this environment's own handlers and give back what
/// they answered — `Value::Null` when nothing handled its class.
///
/// This is what lets a host keep a program *resident*: run it once, hold the
/// environment, and push work into it afterwards. Without it a program is a
/// script that runs and ends, which is fine for a program that owns its
/// process and useless for one being hosted alongside others.
pub fn deliver(particle: &Value, env: &mut Environment) -> Result<Value, String> {
    dispatch_handler(particle, env)
}

/// Hand over everything the environment's linked modules have queued, and
/// answer each. Returns how many were dispatched.
///
/// A host driving several programs calls this instead of `keep_alive`: it
/// decides when each one gets a turn, rather than each one parking on its own
/// thread forever.
pub fn drain(env: &mut Environment) -> Result<usize, String> {
    drain_inbound(env)
}

/// One linked module's half of the inbound conversation: what it has queued,
/// where the program's answer goes, and whether it is still expecting to
/// speak at all.
#[derive(Clone)]
struct InboundSource {
    drain: Rc<dyn Fn() -> Vec<Value>>,
    reply: InboundReply,
    /// `code_module_serving` behind a closure, so this stays free of any
    /// `native-modules`-only type — the same reason `drain` and `reply` are
    /// closures rather than a module handle.
    serving: Rc<dyn Fn() -> bool>,
}

/// Hands one module the answer to a particle it pushed — the pushed particle
/// and the handler's return value, which is null when nothing handled it.
pub type InboundReply = Rc<dyn Fn(&Value, &Value)>;

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
fn register_handlers(
    stmts: &[Stmt],
    env: &mut Environment,
    depth: usize,
    file: usize,
) -> Result<(), String> {
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
                let handler = Rc::new(HandlerBody {
                    fields: fields.clone(),
                    body: body.clone(),
                    defining_depth: depth,
                    file,
                });
                // Every level joins the program-wide table — that is what
                // `to this` and the inbound drain dispatch against — and
                // also its own depth's table, which is what a child's
                // `emit … to base` will look up.
                env.handlers.insert(class_name.clone(), Rc::clone(&handler));
                if env.handler_tables.len() <= depth {
                    env.handler_tables.resize_with(depth + 1, HashMap::new);
                }
                env.handler_tables[depth].insert(class_name.clone(), handler);
            }
            Stmt::Import { body, file, .. } => {
                // Room for the file's world, made before anything can run:
                // handlers are hoisted, so one may be reached before the
                // `link` that would otherwise create it.
                if env.file_scopes.len() <= *file {
                    env.file_scopes.resize_with(file + 1, HashMap::new);
                }
                register_handlers(body, env, depth + 1, *file)?
            }
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
    register_handlers(&program.statements, &mut env, 0, 0)?;
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
    // The last statement is not the end of the program: a module that is
    // still serving holds it open, and pushed particles keep reaching their
    // handlers until nothing does. See `keep_alive`.
    keep_alive(&mut env)?;
    // Last, once nothing can reach them again: anything still linked at
    // runtime is closed, so a guest that was never `unlink`ed still reaches
    // its own release point. `codegen.rs`'s `emit_cleanup` does the same.
    env.unlink_all();
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
                    // Which environment to wake: this one. Set here rather
                    // than when the queue was built, because the queue is
                    // handed to the module before there is an environment for
                    // it to belong to.
                    inbound.wake(env.wakeup());
                    let module = Rc::new(module);
                    let dispatching = Rc::clone(&module);
                    let dispatch: ModuleDispatch = Rc::new(move |v| dispatching.dispatch(v));
                    let asking = Rc::clone(&module);
                    env.link_module(alias, Value::Object(Rc::new(vars)), dispatch);
                    env.link_inbound(
                        Rc::new(move || inbound.take()),
                        Rc::new(move |particle, answer| module.reply(particle, answer)),
                        Rc::new(move || asking.serving()),
                    );
                    Ok(Flow::Normal)
                }
                #[cfg(not(feature = "native-modules"))]
                {
                    let _ = (alias, path);
                    Err("native modules aren't supported in this build".to_string())
                }
            }
        },
        Stmt::LinkRuntime { alias, path } => {
            let path = eval(path, env)?;
            let Value::Str(ref path) = path else {
                return Err(format!("'link' needs a path, found {}", a_type_name(&path)));
            };
            #[cfg(feature = "native-modules")]
            {
                // Only a `.so`. A `.code` source would mean adding handlers
                // while the program runs — deliberately out of scope — and a
                // `.a` is linked into the binary at build time and has
                // nothing to open. Checked on the value rather than in the
                // parser because there is no value until now.
                if !path.ends_with(".so") {
                    return Err(format!(
                        "'link {path}' inside a handler can only open a module ('.so') \
                         — a '.code' source would add handlers while the program runs, and \
                         a '.a' is already part of this binary"
                    ));
                }
                // `dlopen` only treats its argument as a *path* when it
                // contains a slash; a bare name it looks for the way it
                // looks for a shared library, along the loader's search
                // paths — so `link "guest.so"` would quietly miss the file
                // sitting right there and report it as absent. A top-level
                // `link` never runs into this because `loader.rs` has
                // already turned the spelling into a real path before either
                // backend sees it. Here there is no such pass, so "taken as
                // written" has to be made to mean "as a path", which is what
                // a program that just built one out of a directory and a
                // name meant by it. Must match `runtime.c`.
                let path: &str = &if path.contains('/') {
                    path.to_string()
                } else {
                    format!("./{path}")
                };
                let module = NativeModule::open(path)?;
                // A module that speaks first used to be refused here,
                // because the drain ran only over the modules known when the
                // program started. It is listened to now: its queue joins
                // the same list a top-level `link` adds to, and leaves it
                // again on `unlink`. So a door can be chosen while the
                // program runs — which is the point, since an application
                // that may be held cannot know at build time whether it is
                // opening a port or being given a membrane. Must match
                // `runtime.c`'s `code_runtime_link`.
                let inbound = module.has_inbound().then(|| {
                    let queue = module.inbound_handle();
                    // Which environment to wake: this one, the same as for a
                    // top-level link, so one park covers everything.
                    queue.wake(env.wakeup());
                    queue
                });
                // Become its host before anything else touches it: from
                // here on every `link` inside this module asks this
                // program's handlers instead of the filesystem, which is
                // what lets a guest share what the host already has rather
                // than opening its own.
                let env_ptr: *mut Environment = env;
                let guest = unsafe { module.host(path, env_ptr) };
                // How its modules wake this program: the same wakeup
                // this program's own modules signal, so one park covers
                // both and nothing polls.
                crate::native::set_host_wakeup(env.wakeup());
                let module = Rc::new(module);
                let dispatching = Rc::clone(&module);
                let draining = Rc::clone(&module);
                let asking = Rc::clone(&module);
                let inbound = inbound.map(|queue| {
                    let replying = Rc::clone(&module);
                    let serving = Rc::clone(&module);
                    env.link_inbound(
                        Rc::new(move || queue.take()),
                        Rc::new(move |particle, answer| replying.reply(particle, answer)),
                        Rc::new(move || serving.serving()),
                    )
                });
                let module = RuntimeModule {
                    dispatch: Rc::new(move |v| dispatching.dispatch(v)),
                    release: Rc::new(move || module.release()),
                    drain: Rc::new(move || draining.drain()),
                    serving: Rc::new(move || asking.serving()),
                    guest,
                    inbound,
                };
                let address = env.open_module(module);
                env.declare(alias.clone(), address);
                Ok(Flow::Normal)
            }
            #[cfg(not(feature = "native-modules"))]
            {
                let _ = (alias, path);
                Err("native modules aren't supported in this build".to_string())
            }
        }
        Stmt::Unlink(address) => {
            let address = eval(address, env)?;
            env.close_module(&address)?;
            Ok(Flow::Normal)
        }
        Stmt::Import {
            alias,
            body,
            exports,
            file,
        } => {
            // Produce the exported name/value pairs, then bind them. The two
            // halves are kept separate because a native module would supply
            // the pairs from a descriptor instead of from a body, and reuse
            // the binding half unchanged (see `ast::Stmt::Import`).
            //
            // The linking file's world goes home first and the linked one
            // starts empty. That is the direction: what a module exports
            // travels up, and nothing travels down — a module cannot see the
            // names of whoever linked it, and does not know it was linked.
            // `link` is top-level only, so there is exactly one frame to put
            // away here.
            let caller_file = env.current_file;
            let caller_scope = env.scopes.pop().unwrap_or_default();
            env.file_scopes[caller_file] = caller_scope;
            env.current_file = *file;
            env.scopes.push(std::mem::take(&mut env.file_scopes[*file]));

            // Depth bookkeeping for `emit … to base`: statements in the
            // body sit one level further out in the module graph. Decrement
            // on every path — the body may fail.
            env.module_depth += 1;
            let result = exec_body(body, env);
            env.module_depth -= 1;

            // Kept rather than dropped: the file's handlers are still to
            // run, and this is the world they run in.
            let module_scope = env.scopes.pop().unwrap_or_default();
            let pairs = result.and_then(|_| {
                exports
                    .iter()
                    .map(|name| {
                        module_scope
                            .get(name)
                            .cloned()
                            .map(|value| (name.clone(), value))
                            .ok_or_else(|| format!("module exports '{name}' but never defines it"))
                    })
                    .collect::<Result<Vec<_>, _>>()
            });
            env.file_scopes[*file] = module_scope;
            env.current_file = caller_file;
            env.scopes
                .push(std::mem::take(&mut env.file_scopes[caller_file]));
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
                        drain_between_iterations(env)?;
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
                    drain_between_iterations(env)?;
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
                EmitTarget::Base => {
                    // `verify_defined` refused a `to base` outside a linked
                    // module, so `module_depth >= 1` here. The parent's
                    // level may define no handlers at all — then it has no
                    // slot in `handler_tables` either — and that answers
                    // null, the same answer a table lacking the class gives.
                    // Mirrors codegen's `base_dispatches[..] == None`.
                    //
                    // The handler is cloned out of the table *before*
                    // `run_handler` takes `env` mutably — holding the table
                    // borrow across the call would fight the borrow checker
                    // for no reason, since the `Rc` keeps everything alive.
                    let parent = env.handler_tables.get(env.module_depth - 1);
                    let handler = parent.and_then(|table| resolve_handler(table, &value));
                    run_handler(&value, env, handler)?
                }
                EmitTarget::Module(alias) => {
                    // A statically linked alias first — every program that
                    // ran before this existed takes exactly this path. Only
                    // when the name is not one does it become an ordinary
                    // variable holding an address (`Stmt::LinkRuntime`),
                    // which `verify_defined` has already confirmed is bound
                    // somewhere. Must match codegen.rs's `gen_emit`.
                    match env.modules.get(alias) {
                        Some(dispatch) => {
                            let dispatch = dispatch.clone();
                            dispatch(&value)?
                        }
                        None => {
                            let address = env
                                .get(alias)
                                .cloned()
                                .ok_or_else(|| format!("no linked module named '{alias}'"))?;
                            let dispatch = env.module_at(&address)?;
                            dispatch(&value)?
                        }
                    }
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

/// `emit <particle> to this` — runs the handler registered for the
/// particle's own `_class` in the program-wide table.
/// Asks this program's own handlers a question, from outside the program —
/// what a guest's `link` and its `emit`s become when this program is hosting
/// it (`code_abi.h` item 10). `runtime.c`'s `ask_program` is the compiled
/// half of the same thing.
///
/// A failure comes back as an `Exception` rather than an `Err`, because
/// there is nothing above this to hand an `Err` to: the caller is a guest,
/// on the other side of an FFI boundary, and a value is the only thing that
/// can cross it. That is also the answer a handler failing inside this
/// program would already produce.
pub fn ask_program(particle: &Value, env: &mut Environment) -> Value {
    match dispatch_handler(particle, env) {
        Ok(answer) => answer,
        Err(message) => exception(message),
    }
}

/// What a guest gets when it emits to a module its host refused. Must
/// stay word for word identical to `runtime.c`'s: the same program run both
/// ways must answer the same.
pub fn hosting_refusal(name: &str) -> Value {
    host_exception(format!("module '{name}' is not offered by the host"))
}

/// What a guest gets when it emits through a stand-in whose application has
/// been stopped — its handle still names a row, and the row is empty.
pub fn hosting_stopped() -> Value {
    host_exception("this module's application has been stopped".to_string())
}

fn host_exception(message: String) -> Value {
    Value::Object(Rc::new(vec![
        ("_class".to_string(), Value::Str("Exception".into())),
        ("source".to_string(), Value::Str("host".into())),
        ("message".to_string(), Value::Str(message.into())),
        ("innerException".to_string(), Value::Null),
    ]))
}

fn dispatch_handler(particle: &Value, env: &mut Environment) -> Result<Value, String> {
    let handler = resolve_handler(&env.handlers, particle);
    run_handler(particle, env, handler)
}

/// The shared front half of every dispatch path: read the particle's
/// `_class` and look it up in `table`. `None` covers both "not a particle"
/// (no usable class) and "handled by nobody" — both answer null, so one
/// branch serves both. Cloning the `Rc` out here means the caller can drop
/// its borrow of the table before running the body, which may mutate the
/// very environment the table lives in.
fn resolve_handler(
    table: &HashMap<String, Rc<HandlerBody>>,
    particle: &Value,
) -> Option<Rc<HandlerBody>> {
    // `check_emittable` ran at the emit site, so a `_class` is here. A
    // *non-Str* one is not a particle either and takes the same path as an
    // unknown class: null. There is no third answer to give — this function
    // is only ever reached through `emit`.
    let class = match particle {
        Value::Object(fields) => fields.iter().find(|(k, _)| k == "_class").map(|(_, v)| v),
        _ => None,
    };
    let Some(Value::Str(class)) = class else {
        return None;
    };
    // A class nothing handles answers null rather than ending the program:
    // sending a particle is not a demand, and whether to act on one is the
    // recipient's business (decided 2026-08-28, see
    // docs/todo/errors-as-particles.md). The same answer `to core` and a
    // native module give — including here, where the parent simply does not
    // handle the class the child asked about.
    table.get(class.as_ref()).cloned()
}

/// Runs a resolved handler — the back half of `dispatch_handler` and the
/// `to base` arm alike.
fn run_handler(
    particle: &Value,
    env: &mut Environment,
    handler: Option<Rc<HandlerBody>>,
) -> Result<Value, String> {
    let Some(handler) = handler else {
        return Ok(Value::Null);
    };
    // Resolution succeeded, so the particle carried a real class — reading
    // it back is how the re-entry guard below knows what to key on.
    let class = class_of(particle);
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

    // The caller's world steps aside entirely, and the handler's own file
    // takes its place: a handler sees the file it was written in and nothing
    // else, whoever emitted to it and from wherever. The caller's file scope
    // goes home first so that a handler in the *same* file finds the one map
    // its top level declared, rather than a second copy of it.
    //
    // The depth counter steps aside for the same reason: a `to base` inside
    // the body must mean this handler's own parent, not the caller's.
    let mut saved: Vec<HashMap<String, Value>> = std::mem::take(&mut env.scopes);
    let caller_file = env.current_file;
    if !saved.is_empty() {
        env.file_scopes[caller_file] = saved.remove(0);
    }
    env.current_file = handler.file;
    env.scopes
        .push(std::mem::take(&mut env.file_scopes[handler.file]));
    let saved_depth = std::mem::replace(&mut env.module_depth, handler.defining_depth);

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

    // The handler's file keeps whatever the body changed, and the caller
    // gets its own world back.
    env.file_scopes[handler.file] = env.scopes.pop().unwrap_or_default();
    env.current_file = caller_file;
    env.scopes
        .push(std::mem::take(&mut env.file_scopes[caller_file]));
    env.scopes.extend(saved);
    env.module_depth = saved_depth;
    env.active.remove(class);
    result
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
                // A computed key (`{ "$name" = v }`) is an interpolation,
                // and interpolation is total — every value renders — so this
                // is always a Str and never a failure.
                let key = match key {
                    FieldKey::Literal(name) => name.clone(),
                    FieldKey::Computed(expr) => {
                        let rendered = eval(expr, env)?;
                        match &rendered {
                            Value::Str(text) => text.to_string(),
                            other => {
                                return Err(format!(
                                    "a field name must be text, found {}",
                                    a_type_name(other)
                                ))
                            }
                        }
                    }
                };
                values.push((key, eval(value, env)?));
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
        // `expr is X` — a test, not a lookup, and never an error: false is
        // the answer wherever true is not. `X` is one of the six kinds
        // (`String`, `Number`, …) or a particle class; the parser decided
        // which (see `ast::IsTest`).
        Expr::Is(e, test) => {
            let v = eval(e, env)?;
            let answer = match test {
                IsTest::Kind(kind) => kind_of(&v) == *kind,
                // A particle is an object whose `_class` is that name. An
                // object without one, a wrong class, or any non-object value
                // is false rather than an error.
                IsTest::Class(class) => matches!(&v, Value::Object(fields)
                    if fields.iter().any(|(k, val)| k == "_class"
                        && matches!(val, Value::Str(s) if **s == *class))),
            };
            Ok(Value::Bool(answer))
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
        // Whether this run is a module another program linked, rather than a
        // program of its own.
        //
        // Always false here, and not a divergence from `runtime.c`: an
        // interpreted run is a program. Only a `--target shared` build is
        // something a linker reaches into, and it says so in its own
        // start-up.
        //
        // What it is for: the same source can be built both ways, and a few
        // things are only correct in one of them — opening a listening
        // socket of your own above all, since a thread that outlives a
        // module's release point makes it impossible to unload.
        "Linked" => Ok(Value::Object(Rc::new(vec![
            ("_class".to_string(), Value::Str("LinkedResult".into())),
            ("value".to_string(), Value::Bool(false)),
        ]))),
        "Timestamp" => {
            // Whole seconds since the Unix epoch — the old language's
            // `Timestamp` did exactly this, and human-readable formatting
            // belongs in a module, not core (see docs/todo/community-modules.md).
            Ok(core_result("TimestampResult", unix_seconds()))
        }
        // A field the particle does not carry is null — the same answer
        // `.field` gives — so an absent `value` is not a separate case to
        // report. Emitting a particle is not a form to be validated before
        // the handler may run: `Length { }` means `Length { value = null }`,
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

/// `{ _class = class_name, value = n }` — the shape every core handler's
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

/// Whole seconds since the Unix epoch.
///
/// Split by target because `SystemTime::now()` *panics* on
/// `wasm32-unknown-unknown` — there is no clock in the platform, and the
/// playground is that platform. Every `emit Timestamp to core` example on
/// the front page answered `RuntimeError: unreachable` from the day
/// `Timestamp` shipped until 2026-08-30, because nothing ran those examples
/// through the engine the page loads (`site/check_examples.mjs` does now).
///
/// The browser has a clock and hands it over through the same `Date.now()`
/// every other page uses; `js-sys` is a wasm-only dependency, so a native
/// build is untouched by this.
#[cfg(not(target_arch = "wasm32"))]
fn unix_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as f64
}

#[cfg(target_arch = "wasm32")]
fn unix_seconds() -> f64 {
    // Milliseconds, and fractional across the epoch boundary the same way
    // `as_secs` truncates — floor, so the two backends agree on which second
    // it is rather than differing by rounding.
    (js_sys::Date::now() / 1000.0).floor()
}

/// Which of the six kinds a value is — what `is` compares against, and the
/// same enum the error messages name.
fn kind_of(v: &Value) -> ValueKind {
    match v {
        Value::Number(_) => ValueKind::Number,
        Value::Str(_) => ValueKind::String,
        Value::Bool(_) => ValueKind::Boolean,
        Value::Null => ValueKind::Null,
        Value::Array(_) => ValueKind::Array,
        Value::Object(_) => ValueKind::Object,
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
        // A string on either side makes `+` string concatenation: the other
        // operand is rendered exactly as string interpolation would render
        // it (`"n=" + 1` is `"n=1"`, `"ok? " + true` is `"ok? true"`,
        // `"x" + null` is `"xnull"`). The array arms above have already
        // claimed every string-and-array pairing, and string-and-object
        // stays a type error — an object has no bare form to splice — so the
        // guard on each arm excludes both container kinds.
        (BinOp::Add, Str(a), b) if !matches!(b, Array(_) | Object(_)) => {
            Some(Str(Rc::from(format!("{a}{b}").as_str())))
        }
        (BinOp::Add, a, Str(b)) if !matches!(a, Array(_) | Object(_)) => {
            Some(Str(Rc::from(format!("{a}{b}").as_str())))
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

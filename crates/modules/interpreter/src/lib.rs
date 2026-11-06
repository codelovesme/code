//! The `interpreter` native module — source text, loaded as a linked
//! program of its own.
//!
//! Handlers:
//!
//! - `Load { source, name?, host? }` → `Loaded { id }`. The text is parsed
//!   and put through every check a file gets before it runs — handler
//!   cycles, undefined names — and its top-level statements are executed.
//!   Any of that failing is an `Exception { message }`: the source is bad,
//!   and this is where it is said. A `link` inside the source is refused: a
//!   program loaded here links nothing.
//!
//!   With `host = true` the program may `emit … to <name>` its base's
//!   modules without linking them (`code_abi.h` item 14). Every name it
//!   emits to is asked of the base first, as a hosted guest's link is:
//!   `Offer { app, name }`, answered `Offered` — and a name not offered
//!   refuses the whole program before any of it runs. Each emit then
//!   reaches the base as `Module { app, name, particle }`, nested, and its
//!   answer is the emit's. `app` is the program's `name`. Without `host`
//!   nothing changes.
//! - `Check { source, name?, host? }` → `Checked { ok, hash, modules,
//!   offered, missing, stage?, message? }`: what `Load` would examine,
//!   nothing run — the base is asked only `Offer`.
//! - `Replace { id, source, version?, host? }` → `Replaced { id, version,
//!   hash }`. The new version is built whole (checked, bound, top level
//!   run) before it takes the old one's place; any refusal leaves the old
//!   one answering. `version`, when given, must be the current one.
//! - `Send { id, particle }` → whatever the handler answered; `null` when
//!   no handler takes the class; an `Exception` when the handler failed,
//!   exactly as a runtime error answers anywhere.
//! - `Stop { id }` / `Start { id }` → `Stopped`/`Started { id, was }`. A
//!   stopped program keeps its state and refuses every `Send`.
//! - `Unload { id }` → `Unloaded { existed }`.
//! - `Status { id }` → `Status { id, name, version, hash, state, busy,
//!   modules, calls, failures, errors }`, the newest 16 failures.
//! - `Loaded {}` → `Programs { items = [{ id, name, version, hash, state,
//!   modules, failures }] }`.
//!
//! A refusal is an `Exception` with a `stage` — `request`, `source`,
//! `binding` (with `missing`), `activation`, `absent`, `stopped`, `busy`,
//! `stale` — and the program's `id` when there is one.
//!
//! # What a loaded program is
//!
//! A linked module the base made itself, from text, while running. It has
//! its own handlers and its own top level, it sees nothing of the base's,
//! and it is talked to with particles — the standing a `.so` has, without
//! the file. What it cannot do is reach back: it links no modules, so the
//! world it knows is what each particle brings it. That is the point for
//! rules, plugins, and anything else a person may write into a running
//! program: the base decides what goes in and what to do with what comes
//! out.
//!
//! # Where it works
//!
//! A machine. The page has the interpreter already (`crates/code-wasm`).
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use code::ast::NativeFormat;
use code::interpreter::{ask_program, prepare, Environment, ModuleDispatch};
use code::loader::{self, ModuleResolver, NoModules, ResolvedModule};
use code::value::Value;
use code_native::*;
use sha2::{Digest, Sha256};

/// The most source one `Load` takes. A rule is a screenful; a megabyte is
/// a mistake, and a mistake should be refused before it is parsed.
const SOURCE_AT_MOST: usize = 256 * 1024;

/// How many failures a program remembers, newest kept; and how long one
/// failure's message may be. A program that fails every second must not
/// grow without bound, and a message is a reason, not a payload.
const FAILURES_KEPT: usize = 16;
const MESSAGE_AT_MOST: usize = 400;

/// One failure, as `Status` reports it. Only what went wrong and where —
/// never the particle that was sent, which may carry anything.
struct Failure {
    seq: u64,
    version: u64,
    operation: &'static str,
    target: String,
    message: String,
}

#[derive(Default)]
struct Log {
    calls: u64,
    failures: u64,
    recent: VecDeque<Failure>,
}

impl Log {
    fn note(&mut self, version: u64, operation: &'static str, target: &str, message: &str) {
        self.failures += 1;
        if self.recent.len() == FAILURES_KEPT {
            self.recent.pop_front();
        }
        self.recent.push_back(Failure {
            seq: self.failures,
            version,
            operation,
            target: target.to_string(),
            message: cut(message, MESSAGE_AT_MOST),
        });
    }
}

/// What one version of a program is once it has started: its world and
/// the facts about the source it came from.
struct Built {
    modules: Vec<String>,
    hash: String,
    /// Boxed so the environment never moves once made — the same rule
    /// `interpreter::run_with` keeps, and for the same reason.
    env: Box<Environment>,
}

struct Loaded {
    name: String,
    /// Counts up from 1 with every `Replace` that took.
    version: u64,
    hash: String,
    /// The base's modules this program reaches (`host = true`), sorted.
    modules: Vec<String>,
    hosted: bool,
    stopped: bool,
    /// `None` while a `Send` is inside the program; the rest of the record
    /// stays in the table so `Status`, `Stop` and `Unload` still find it.
    env: Option<Box<Environment>>,
    /// Unloaded while a `Send` was inside it: dropped when that returns.
    gone: bool,
    log: Rc<RefCell<Log>>,
}

thread_local! {
    // A program's environment holds `Rc`s and is talked to on the one
    // thread that dispatches into this module, so the table is that
    // thread's own. Ids count up and are never reused.
    static PROGRAMS: RefCell<BTreeMap<u64, Loaded>> = const { RefCell::new(BTreeMap::new()) };
    static NEXT_ID: RefCell<u64> = const { RefCell::new(1) };
}

/// Why a request was refused, carried on the `Exception` it answers:
/// `stage` says which kind of thing went wrong.
///
/// - `request` — the particle itself was malformed.
/// - `source` — the text does not hold up (parse, undefined names, `link`).
/// - `binding` — a name it emits to is not offered; `missing` lists them.
/// - `activation` — its top level failed while starting.
/// - `absent`, `stopped`, `busy`, `stale` — the program's state refused it.
struct Refusal {
    stage: &'static str,
    message: String,
    missing: Vec<String>,
    id: Option<u64>,
}

impl Refusal {
    fn new(stage: &'static str, message: impl Into<String>) -> Self {
        Refusal {
            stage,
            message: message.into(),
            missing: Vec::new(),
            id: None,
        }
    }

    fn of(mut self, id: u64) -> Self {
        self.id = Some(id);
        self
    }

    fn value(&self) -> Value {
        let mut fields = vec![
            ("_class".to_string(), Value::Str("Exception".into())),
            ("source".to_string(), Value::Str("interpreter".into())),
            (
                "message".to_string(),
                Value::Str(self.message.as_str().into()),
            ),
            ("innerException".to_string(), Value::Null),
            ("stage".to_string(), Value::Str(self.stage.into())),
        ];
        if !self.missing.is_empty() {
            fields.push((
                "missing".to_string(),
                Value::Array(Rc::new(
                    self.missing
                        .iter()
                        .map(|m| Value::Str(m.as_str().into()))
                        .collect(),
                )),
            ));
        }
        if let Some(id) = self.id {
            fields.push(("id".to_string(), Value::Number(id as f64)));
        }
        Value::Object(Rc::new(fields.into()))
    }
}

impl From<String> for Refusal {
    fn from(message: String) -> Self {
        Refusal::new("request", message)
    }
}

impl From<&str> for Refusal {
    fn from(message: &str) -> Self {
        Refusal::new("request", message)
    }
}

// Asked by a host that can be this module's base (`code_abi.h` item 14).
declare_base!();

#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// # Safety
///
/// Both pointers must be valid for the duration of the call and laid out
/// per `code_abi.h` — the host guarantees this on every dispatch.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "interpreter", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Load" => load(particle),
            "Check" => check(particle),
            "Replace" => replace(particle),
            "Send" => return send(out, particle),
            "Stop" => stop(particle, true),
            "Start" => stop(particle, false),
            "Unload" => unload(particle),
            "Status" => status(particle),
            "Loaded" => Ok(loaded()),
            _ => Ok(Value::Null),
        };
        match outcome {
            Ok(answer) => from_value(out, &answer),
            Err(refusal) => from_value(out, &refusal.value()),
        }
    })
}

/// The program's text, the name it goes by and whether it is hosted, read
/// off a `Load`, `Check` or `Replace`.
struct Request<'a> {
    source: &'a str,
    name: String,
    hosted: bool,
}

fn request<'a>(particle: &'a CodeValue, what: &str) -> Result<Request<'a>, Refusal> {
    let source = read_field_str(particle, "source")
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{what} requires a non-empty string 'source'"))?;
    if source.len() > SOURCE_AT_MOST {
        return Err(format!(
            "{what} refuses {} bytes of source — the most it takes is {SOURCE_AT_MOST}",
            source.len()
        )
        .into());
    }
    Ok(Request {
        source,
        name: read_field_str(particle, "name").unwrap_or("").to_string(),
        hosted: read_field_bool(particle, "host").unwrap_or(false),
    })
}

fn identity(name: &str) -> String {
    if name.is_empty() {
        "<loaded>".to_string()
    } else {
        format!("<{name}>")
    }
}

fn app_name(name: &str) -> &str {
    if name.is_empty() {
        "loaded"
    } else {
        name
    }
}

/// Everything short of running: the parsed program, what it emits to, and
/// which of those the base offers. Nothing of the source has run and the
/// base was asked only `Offer`.
struct Checked {
    program: code::ast::Program,
    modules: Vec<String>,
    missing: Vec<String>,
}

fn examine(req: &Request) -> Result<Checked, Refusal> {
    let identity = identity(&req.name);
    let alone = NoModules {
        entry_identity: identity.clone(),
        entry_text: req.source.to_string(),
    };
    if !req.hosted {
        let program = loader::load(&identity, &alone).map_err(|m| Refusal::new("source", m))?;
        code::verify::verify_defined(&program).map_err(|m| Refusal::new("source", m))?;
        return Ok(Checked {
            program,
            modules: Vec::new(),
            missing: Vec::new(),
        });
    }
    // What it emits to, read off the source before anything runs, and
    // every one of those asked of the base.
    let first = loader::load(&identity, &alone).map_err(|m| Refusal::new("source", m))?;
    let modules = code::introspection::module_targets(&first);
    let app = app_name(&req.name);
    let missing: Vec<String> = modules
        .iter()
        .filter(|m| !offered(app, m))
        .cloned()
        .collect();
    // An entry of its own that links each of them under its own name,
    // then the program as a file of its own — so its handlers and its
    // line numbers are the source's, as a gene's are.
    let mut entry = String::new();
    for m in &modules {
        entry.push_str(&format!("link \"{m}\" as {m}\n"));
    }
    entry.push_str(&format!("link \"{identity}\"\n"));
    let furnished = Furnished {
        entry_text: entry,
        program_identity: identity.clone(),
        program_text: req.source.to_string(),
        modules: modules.clone(),
    };
    let program = loader::load("<base>", &furnished).map_err(|m| Refusal::new("source", m))?;
    code::verify::verify_defined(&program).map_err(|m| Refusal::new("source", m))?;
    Ok(Checked {
        program,
        modules,
        missing,
    })
}

/// Checks, then starts: the program's top level runs here, so whatever it
/// emits has happened by the time this answers — a failure after that is
/// an `activation` refusal, and its effects are not undone.
fn build(req: &Request, version: u64, log: &Rc<RefCell<Log>>) -> Result<Built, Refusal> {
    let checked = examine(req)?;
    if !checked.missing.is_empty() {
        let mut refusal = Refusal::new(
            "binding",
            format!(
                "the host does not offer {} to this program: {}",
                if checked.missing.len() == 1 {
                    "a module"
                } else {
                    "modules"
                },
                checked.missing.join(", ")
            ),
        );
        refusal.missing = checked.missing;
        return Err(refusal);
    }
    let env = Box::new(Environment::default());
    if req.hosted {
        let app: Rc<str> = Rc::from(app_name(&req.name));
        for m in &checked.modules {
            let app = Rc::clone(&app);
            let module: Rc<str> = Rc::from(m.as_str());
            let log = Rc::clone(log);
            // Each version's calls carry that version: a failure is
            // reported against the program that made it, even after a
            // `Replace` has put another in its place.
            let dispatch: ModuleDispatch = Rc::new(move |sent: &Value| {
                let answer = to_base(&app, &module, sent);
                if let Some(message) = exception_message(&answer) {
                    log.borrow_mut().note(version, "Module", &module, &message);
                }
                Ok(answer)
            });
            env.provide_module(m, Value::Object(Rc::new(Vec::new().into())), dispatch);
        }
    }
    prepare(&checked.program, &env).map_err(|m| Refusal::new("activation", m))?;
    Ok(Built {
        modules: checked.modules,
        hash: hash(req.source),
        env,
    })
}

fn load(particle: &CodeValue) -> Result<Value, Refusal> {
    let req = request(particle, "Load")?;
    let log = Rc::new(RefCell::new(Log::default()));
    let built = build(&req, 1, &log)?;
    let id = NEXT_ID.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n += 1;
        id
    });
    let hash = built.hash.clone();
    PROGRAMS.with(|p| {
        p.borrow_mut().insert(
            id,
            Loaded {
                name: req.name,
                version: 1,
                hash: built.hash,
                modules: built.modules,
                hosted: req.hosted,
                stopped: false,
                env: Some(built.env),
                gone: false,
                log,
            },
        )
    });
    Ok(object_value(
        "Loaded",
        vec![
            ("id", Value::Number(id as f64)),
            ("version", Value::Number(1.0)),
            ("hash", Value::Str(hash.as_str().into())),
        ],
    ))
}

/// The same examination `Load` makes, without running anything: what the
/// source emits to, which of those the base offers, and — when it does not
/// hold up — why, at which stage.
fn check(particle: &CodeValue) -> Result<Value, Refusal> {
    let req = request(particle, "Check")?;
    let strs = |items: &[String]| {
        Value::Array(Rc::new(
            items
                .iter()
                .map(|m| Value::Str(m.as_str().into()))
                .collect(),
        ))
    };
    let hash = Value::Str(hash(req.source).as_str().into());
    Ok(match examine(&req) {
        Ok(checked) => {
            let offered: Vec<String> = checked
                .modules
                .iter()
                .filter(|m| !checked.missing.contains(m))
                .cloned()
                .collect();
            let ok = checked.missing.is_empty();
            let mut fields = vec![
                ("ok", Value::Bool(ok)),
                ("hash", hash),
                ("modules", strs(&checked.modules)),
                ("offered", strs(&offered)),
                ("missing", strs(&checked.missing)),
            ];
            if !ok {
                fields.push(("stage", Value::Str("binding".into())));
                fields.push((
                    "message",
                    Value::Str(
                        format!("not offered: {}", checked.missing.join(", "))
                            .as_str()
                            .into(),
                    ),
                ));
            }
            object_value("Checked", fields)
        }
        Err(refusal) => object_value(
            "Checked",
            vec![
                ("ok", Value::Bool(false)),
                ("hash", hash),
                ("stage", Value::Str(refusal.stage.into())),
                ("message", Value::Str(refusal.message.as_str().into())),
                ("modules", strs(&[])),
                ("offered", strs(&[])),
                ("missing", strs(&[])),
            ],
        ),
    })
}

fn id_of(particle: &CodeValue, what: &str) -> Result<u64, Refusal> {
    Ok(read_field_number(particle, "id")
        .filter(|n| *n >= 1.0 && n.fract() == 0.0)
        .ok_or_else(|| format!("{what} requires a number 'id' from Load"))? as u64)
}

fn absent(id: u64) -> Refusal {
    Refusal::new("absent", format!("no program is loaded as {id}")).of(id)
}

/// A new source for a loaded program. The new version is built whole —
/// checked, bound, its top level run — before it takes the old one's place;
/// until that point the old version is untouched and keeps answering. Any
/// refusal leaves it exactly as it was.
fn replace(particle: &CodeValue) -> Result<Value, Refusal> {
    let id = id_of(particle, "Replace")?;
    let mut req = request(particle, "Replace")?;
    let expected = read_field_number(particle, "version");
    let (name, hosted, version, log) = PROGRAMS.with(|p| {
        let p = p.borrow();
        let Some(loaded) = p.get(&id).filter(|l| !l.gone) else {
            return Err(absent(id));
        };
        if loaded.env.is_none() {
            return Err(Refusal::new(
                "busy",
                format!("program {id} is answering a Send and cannot be replaced from inside it"),
            )
            .of(id));
        }
        if let Some(expected) = expected {
            if expected != loaded.version as f64 {
                return Err(Refusal::new(
                    "stale",
                    format!(
                        "program {id} is at version {}, not {expected}",
                        loaded.version
                    ),
                )
                .of(id));
            }
        }
        Ok((
            loaded.name.clone(),
            loaded.hosted,
            loaded.version,
            Rc::clone(&loaded.log),
        ))
    })?;
    // The name and the hosting are the program's, not the new text's: a
    // `Replace` changes what it does, not who it is to the base.
    req.name = name;
    if read_field_bool(particle, "host").is_none() {
        req.hosted = hosted;
    }
    let next = version + 1;
    let built = match build(&req, next, &log) {
        Ok(built) => built,
        Err(refusal) => {
            log.borrow_mut()
                .note(version, "Replace", refusal.stage, &refusal.message);
            return Err(refusal.of(id));
        }
    };
    // Building ran the new top level, which may have reached the base, and
    // the base may have unloaded or replaced this program meanwhile.
    PROGRAMS.with(|p| {
        let mut p = p.borrow_mut();
        let Some(loaded) = p.get_mut(&id).filter(|l| !l.gone) else {
            return Err(absent(id));
        };
        if loaded.version != version || loaded.env.is_none() {
            return Err(Refusal::new(
                "stale",
                format!("program {id} changed while version {next} was starting"),
            )
            .of(id));
        }
        loaded.version = next;
        loaded.hash = built.hash.clone();
        loaded.modules = built.modules;
        loaded.hosted = req.hosted;
        loaded.env = Some(built.env);
        Ok(())
    })?;
    Ok(object_value(
        "Replaced",
        vec![
            ("id", Value::Number(id as f64)),
            ("version", Value::Number(next as f64)),
            ("hash", Value::Str(built.hash.as_str().into())),
        ],
    ))
}

fn send(out: &mut CodeValue, particle: &CodeValue) {
    let answer = match deliver(particle) {
        Ok(answer) => answer,
        Err(refusal) => refusal.value(),
    };
    from_value(out, &answer);
}

fn deliver(particle: &CodeValue) -> Result<Value, Refusal> {
    let id = id_of(particle, "Send")?;
    let sent = find_field(particle, "particle").ok_or("Send requires a 'particle'")?;
    if sent.tag != CodeTag::Object {
        return Err("Send's 'particle' must be an object with a '_class'".into());
    }
    let value = to_value(sent);
    // The environment is taken out of its record for the call, and the
    // record stays: a hosted program's handler can reach the base, and the
    // base can ask this module about the program meanwhile — `Status`,
    // `Stop`, `Unload` all still find it; a second `Send` into it is
    // refused as `busy` rather than turned into a panic.
    let (env, version, log) = PROGRAMS.with(|p| {
        let mut p = p.borrow_mut();
        let Some(loaded) = p.get_mut(&id).filter(|l| !l.gone) else {
            return Err(absent(id));
        };
        if loaded.stopped {
            return Err(Refusal::new("stopped", format!("program {id} is stopped")).of(id));
        }
        let Some(env) = loaded.env.take() else {
            return Err(
                Refusal::new("busy", format!("program {id} is already answering a Send")).of(id),
            );
        };
        Ok((env, loaded.version, Rc::clone(&loaded.log)))
    })?;
    let answer = ask_program(&value, &env);
    {
        let mut log = log.borrow_mut();
        log.calls += 1;
        if let Some(message) = exception_message(&answer) {
            let class = match &value {
                Value::Object(fields) => fields
                    .iter()
                    .find(|(k, _)| k == "_class")
                    .and_then(|(_, v)| match v {
                        Value::Str(c) => Some(c.to_string()),
                        _ => None,
                    })
                    .unwrap_or_default(),
                _ => String::new(),
            };
            log.note(version, "Send", &class, &message);
        }
    }
    PROGRAMS.with(|p| {
        let mut p = p.borrow_mut();
        let drop_it = match p.get_mut(&id) {
            Some(loaded) if loaded.gone => true,
            Some(loaded) => {
                loaded.env = Some(env);
                false
            }
            None => false,
        };
        if drop_it {
            p.remove(&id);
        }
    });
    Ok(answer)
}

/// `Stop` keeps the program — its top level's values, its record, its
/// failures — and refuses every `Send` until `Start`. Both are idempotent:
/// `was` says what it was before.
fn stop(particle: &CodeValue, stopping: bool) -> Result<Value, Refusal> {
    let what = if stopping { "Stop" } else { "Start" };
    let id = id_of(particle, what)?;
    let was = PROGRAMS.with(|p| {
        let mut p = p.borrow_mut();
        let Some(loaded) = p.get_mut(&id).filter(|l| !l.gone) else {
            return Err(absent(id));
        };
        let was = loaded.stopped;
        loaded.stopped = stopping;
        Ok(was)
    })?;
    Ok(object_value(
        if stopping { "Stopped" } else { "Started" },
        vec![
            ("id", Value::Number(id as f64)),
            ("was", Value::Str(state_name(was).into())),
        ],
    ))
}

fn unload(particle: &CodeValue) -> Result<Value, Refusal> {
    let id = id_of(particle, "Unload")?;
    // A program answering a `Send` is marked and dropped when that returns:
    // its environment is in use right now, further up this very thread.
    let existed = PROGRAMS.with(|p| {
        let mut p = p.borrow_mut();
        match p.get_mut(&id) {
            Some(loaded) if loaded.gone => false,
            Some(loaded) if loaded.env.is_none() => {
                loaded.gone = true;
                true
            }
            Some(_) => p.remove(&id).is_some(),
            None => false,
        }
    });
    Ok(object_value(
        "Unloaded",
        vec![("existed", Value::Bool(existed))],
    ))
}

fn state_name(stopped: bool) -> &'static str {
    if stopped {
        "stopped"
    } else {
        "running"
    }
}

fn summary(id: u64, l: &Loaded) -> Vec<(&'static str, Value)> {
    let log = l.log.borrow();
    vec![
        ("id", Value::Number(id as f64)),
        ("name", Value::Str(l.name.as_str().into())),
        ("version", Value::Number(l.version as f64)),
        ("hash", Value::Str(l.hash.as_str().into())),
        ("state", Value::Str(state_name(l.stopped).into())),
        (
            "modules",
            Value::Array(Rc::new(
                l.modules
                    .iter()
                    .map(|m| Value::Str(m.as_str().into()))
                    .collect(),
            )),
        ),
        ("calls", Value::Number(log.calls as f64)),
        ("failures", Value::Number(log.failures as f64)),
    ]
}

fn status(particle: &CodeValue) -> Result<Value, Refusal> {
    let id = id_of(particle, "Status")?;
    PROGRAMS.with(|p| {
        let p = p.borrow();
        let Some(loaded) = p.get(&id).filter(|l| !l.gone) else {
            return Err(absent(id));
        };
        let mut fields = summary(id, loaded);
        fields.push(("busy", Value::Bool(loaded.env.is_none())));
        let recent = loaded
            .log
            .borrow()
            .recent
            .iter()
            .map(|f| {
                object_value(
                    "Failure",
                    vec![
                        ("seq", Value::Number(f.seq as f64)),
                        ("version", Value::Number(f.version as f64)),
                        ("operation", Value::Str(f.operation.into())),
                        ("target", Value::Str(f.target.as_str().into())),
                        ("message", Value::Str(f.message.as_str().into())),
                    ],
                )
            })
            .collect();
        fields.push(("errors", Value::Array(Rc::new(recent))));
        Ok(object_value("Status", fields))
    })
}

fn loaded() -> Value {
    let items = PROGRAMS.with(|p| {
        p.borrow()
            .iter()
            .filter(|(_, l)| !l.gone)
            .map(|(id, l)| {
                let mut fields = summary(*id, l);
                fields.retain(|(k, _)| *k != "calls");
                Value::Object(Rc::new(
                    fields
                        .into_iter()
                        .map(|(k, v)| (k.to_string(), v))
                        .collect(),
                ))
            })
            .collect()
    });
    object_value("Programs", vec![("items", Value::Array(Rc::new(items)))])
}

fn object_value(class: &str, fields: Vec<(&str, Value)>) -> Value {
    let mut all = vec![("_class".to_string(), Value::Str(class.into()))];
    all.extend(fields.into_iter().map(|(k, v)| (k.to_string(), v)));
    Value::Object(Rc::new(all.into()))
}

/// `sha256:` and the hex digest of the source's bytes: which text is
/// running, as a fact that does not depend on any name.
fn hash(source: &str) -> String {
    let digest = Sha256::digest(source.as_bytes());
    let mut out = String::from("sha256:");
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn exception_message(v: &Value) -> Option<String> {
    let Value::Object(fields) = v else {
        return None;
    };
    let class = fields.iter().find(|(k, _)| k == "_class")?;
    if !matches!(&class.1, Value::Str(c) if &**c == "Exception") {
        return None;
    }
    Some(
        fields
            .iter()
            .find(|(k, _)| k == "message")
            .and_then(|(_, v)| match v {
                Value::Str(m) => Some(m.to_string()),
                _ => None,
            })
            .unwrap_or_default(),
    )
}

fn cut(text: &str, at_most: usize) -> String {
    if text.len() <= at_most {
        return text.to_string();
    }
    let mut end = at_most;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

/// The module story of a hosted program: its entry links each furnished
/// name (answered by the base, through `provide_module`) and then the
/// program's own source; anything else is refused, as for every program
/// loaded here.
struct Furnished {
    entry_text: String,
    program_identity: String,
    program_text: String,
    modules: Vec<String>,
}

impl ModuleResolver for Furnished {
    fn resolve_entry(&self, _entry: &str) -> Result<ResolvedModule, String> {
        Ok(ResolvedModule::Source {
            identity: "<base>".to_string(),
            text: self.entry_text.clone(),
        })
    }

    fn resolve(&self, _from: &str, module_ref: &str) -> Result<ResolvedModule, String> {
        if module_ref == self.program_identity {
            return Ok(ResolvedModule::Source {
                identity: self.program_identity.clone(),
                text: self.program_text.clone(),
            });
        }
        if self.modules.iter().any(|m| m == module_ref) {
            return Ok(ResolvedModule::Native {
                identity: module_ref.to_string(),
                path: module_ref.to_string(),
                format: NativeFormat::JsBridge,
            });
        }
        Err(format!(
            "cannot link '{module_ref}': a program loaded here links nothing"
        ))
    }
}

/// Asks the base, and keeps its answer as this side's own value.
fn ask(particle: &Value) -> Option<Value> {
    let mut sent = CodeValue::zeroed();
    from_value(&mut sent, particle);
    let answer = ask_base(&sent, to_value);
    release(&mut sent);
    answer
}

fn hosting(class: &str, app: &str, name: &str, particle: Option<&Value>) -> Value {
    let mut fields = vec![
        ("_class".to_string(), Value::Str(class.into())),
        ("app".to_string(), Value::Str(app.into())),
        ("name".to_string(), Value::Str(name.into())),
    ];
    if let Some(particle) = particle {
        fields.push(("particle".to_string(), particle.clone()));
    }
    Value::Object(Rc::new(fields.into()))
}

/// Whether the base offers `name` to the program `app`: only `Offered`
/// counts. Silence is a refusal here — a loaded program has no files of its
/// own to fall back to, unlike a guest.
fn offered(app: &str, name: &str) -> bool {
    matches!(
        &ask(&hosting("Offer", app, name, None)),
        Some(Value::Object(fields))
            if fields.iter().any(|(k, v)| k == "_class" && matches!(v, Value::Str(c) if &**c == "Offered"))
    )
}

/// One emit to a furnished name: the base's `Module` handler answers it.
fn to_base(app: &str, name: &str, particle: &Value) -> Value {
    match ask(&hosting("Module", app, name, Some(particle))) {
        Some(answer) => answer,
        None => exception_value(&format!(
            "module '{name}' cannot be reached: this host cannot be asked"
        )),
    }
}

fn exception_value(message: &str) -> Value {
    Value::Object(Rc::new(
        vec![
            ("_class".to_string(), Value::Str("Exception".into())),
            ("source".to_string(), Value::Str("interpreter".into())),
            ("message".to_string(), Value::Str(message.into())),
        ]
        .into(),
    ))
}

// ---------------------------------------------------------------------------
// Across the boundary: the module's values and the interpreter's.
//
// A particle arrives as a `CodeValue` built by the base's runtime and goes
// into the loaded program as the interpreter's own `Value`; the answer
// comes back the other way. Nothing is shared: both directions copy.
// ---------------------------------------------------------------------------

fn to_value(v: &CodeValue) -> Value {
    match v.tag {
        CodeTag::Number => Value::Number(v.number()),
        CodeTag::Str => Value::Str(read_str(v).unwrap_or("").into()),
        CodeTag::Bool => Value::Bool(v.boolean() != 0),
        CodeTag::Null => Value::Null,
        CodeTag::Array => Value::Array(Rc::new(array_elems(v).map(to_value).collect())),
        CodeTag::Object => Value::Object(Rc::new(
            object_entries(v)
                .map(|(k, item)| (k.to_string(), to_value(item)))
                .collect(),
        )),
    }
}

fn from_value(out: &mut CodeValue, v: &Value) {
    match v {
        Value::Number(n) => number(out, *n),
        Value::Str(s) => owned_str(out, s),
        Value::Bool(b) => boolean(out, *b),
        Value::Null => null(out),
        Value::Array(items) => {
            let mut elems = SlotBuffer::new(items.len());
            for (i, item) in items.iter().enumerate() {
                from_value(elems.slot_mut(i as i64), item);
            }
            array(out, &mut elems);
            elems.release_all();
        }
        Value::Object(fields) => {
            let keys: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
            let mut values = SlotBuffer::new(fields.len());
            for (i, (_, item)) in fields.iter().enumerate() {
                from_value(values.slot_mut(i as i64), item);
            }
            object_dyn(out, &keys, &mut values);
            values.release_all();
        }
    }
}

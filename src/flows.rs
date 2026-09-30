//! What one interaction did, handed to the program's own `Trace` handler.
//!
//! A program that defines `Trace` is given one particle after each
//! *interaction* — everything that happened because of one thing arriving at
//! the top of the program: a top-level `emit`, or a particle a module pushed.
//! That first boundary is the *root*; every boundary crossed while it runs is
//! one of its steps. A program without the handler records nothing.
//!
//! This is not `crate::trace`. That one is a deterministic, replayable record
//! of a whole run, for tests; this one is timed, per interaction, and kept
//! only until it is delivered. They hook in at the same places and share
//! nothing else. The design, and why it is a handler rather than a flag, a
//! file or a module, is `docs/todo/trace-handler.md`.

use std::rc::Rc;

use crate::value::Value;

/// The handler a program defines to be given its traces, and the class of
/// the particle it is given.
pub const HANDLER: &str = "Trace";

/// How many steps one interaction keeps. Past it they are counted, not kept,
/// so a runaway loop cannot turn one trace into the whole of memory.
pub const MOST_STEPS: usize = 500;

/// A boundary that was opened, for `finish` to close. `None` inside means it
/// was past the cap and is only being counted.
#[derive(Debug, Clone, Copy)]
pub struct Opened {
    id: Option<usize>,
    root: bool,
}

#[derive(Debug)]
struct Step {
    parent: Option<usize>,
    target: String,
    class: String,
    at: f64,
    ms: f64,
    ok: bool,
    /// Which held program it ran in — the path it was linked from — or
    /// `None` for this program's own step. A host fills it in when it splices
    /// a guest's steps into its own interaction.
    program: Option<Value>,
}

#[derive(Debug)]
struct Interaction {
    /// Unix milliseconds when the root arrived.
    started: f64,
    /// The monotonic clock at the same moment; `at` and `ms` are read
    /// against it, so a wall-clock jump cannot make a step take negative time.
    clock: f64,
    from: String,
    particle: Value,
    steps: Vec<Step>,
    /// The steps currently running, innermost last: what a new step is
    /// inside of.
    open: Vec<usize>,
    dropped: usize,
}

/// The recorder: the interaction that is running, if one is. Present on an
/// environment only when the program defines [`HANDLER`].
#[derive(Debug, Default)]
pub struct Flows {
    current: Option<Interaction>,
    /// Set while the `Trace` handler runs, so delivering a trace never
    /// records another one.
    delivering: bool,
    /// Whole interactions a held program had on its own — a timer of its
    /// own firing, its top level running — waiting to be handed to `Trace`
    /// the moment nothing else is running.
    pending: Vec<Value>,
}

impl Flows {
    pub fn new() -> Self {
        Flows::default()
    }

    /// Opens a boundary. `root_from` is where it came from when it is at the
    /// top of the program — `this` for a top-level statement, a module's
    /// alias for a particle that module pushed — and `None` when something
    /// is already running. A root with an interaction still open (which the
    /// callers never produce) is recorded as a step of it rather than
    /// losing either.
    pub fn begin(
        &mut self,
        target: &str,
        particle: &Value,
        root_from: Option<&str>,
    ) -> Option<Opened> {
        if self.delivering {
            return None;
        }
        let root = match (&self.current, root_from) {
            (None, Some(from)) => {
                self.current = Some(Interaction {
                    started: unix_ms(),
                    clock: monotonic_ms(),
                    from: from.to_string(),
                    particle: particle.clone(),
                    steps: Vec::new(),
                    open: Vec::new(),
                    dropped: 0,
                });
                true
            }
            (None, None) => return None,
            (Some(_), _) => false,
        };
        let interaction = self.current.as_mut()?;
        if interaction.steps.len() >= MOST_STEPS {
            interaction.dropped += 1;
            return Some(Opened { id: None, root });
        }
        let id = interaction.steps.len();
        interaction.steps.push(Step {
            parent: interaction.open.last().copied(),
            target: target.to_string(),
            class: crate::trace::class_of(particle).to_string(),
            at: monotonic_ms() - interaction.clock,
            ms: 0.0,
            ok: true,
            program: None,
        });
        interaction.open.push(id);
        Some(Opened { id: Some(id), root })
    }

    /// Closes a boundary. `ok` is false when it answered an Exception or
    /// failed outright. For the root, the interaction is over: its `Trace`
    /// particle comes back, and the recorder holds nothing any more.
    pub fn finish(&mut self, opened: Opened, ok: bool) -> Option<Value> {
        let interaction = self.current.as_mut()?;
        if let Some(id) = opened.id {
            let now = monotonic_ms() - interaction.clock;
            if let Some(step) = interaction.steps.get_mut(id) {
                step.ms = now - step.at;
                step.ok = ok;
            }
            if interaction.open.last() == Some(&id) {
                interaction.open.pop();
            }
        }
        if !opened.root {
            return None;
        }
        let done = self.current.take()?;
        Some(done.into_particle())
    }

    /// Brackets the `Trace` handler's own run.
    pub fn set_delivering(&mut self, delivering: bool) {
        self.delivering = delivering;
    }

    /// What a held program reported after this program called into it or
    /// emptied its queues: the traces it finished meanwhile, each stamped
    /// with `program` (the path it was linked from).
    ///
    /// The one whose root came `from` `host` is this program's own call
    /// seen from the inside, so it is spliced into the running interaction
    /// under `under` — the step that made the call — and its root becomes
    /// that step's child. Anything else is an interaction the guest had on
    /// its own, and waits in `pending` to be delivered as one.
    pub fn absorb(&mut self, traces: Vec<Value>, program: &Value, under: Option<Opened>) {
        for trace in traces {
            let from_host = text(&field(&field(&trace, "root"), "from")) == "host";
            match (from_host, under.and_then(|o| o.id), self.current.as_mut()) {
                (true, Some(parent), Some(interaction)) => {
                    interaction.splice(parent, &trace, program)
                }
                // A call this program made while it was not recording — the
                // `Trace` handler itself, shipping a trace to a held
                // library — is dropped. Queued, it would come back as a
                // trace, be shipped, and make another, forever.
                (true, _, _) => {}
                _ => self.pending.push(stamp(trace, program)),
            }
        }
    }

    /// The next held program's interaction waiting for `Trace`, when none of
    /// this program's own is running.
    pub fn next_pending(&mut self) -> Option<Value> {
        if self.current.is_some() || self.pending.is_empty() {
            return None;
        }
        Some(self.pending.remove(0))
    }
}

impl Interaction {
    /// Adds a held program's steps under `parent`, renumbered into this
    /// interaction and moved onto its clock. Past the cap they are counted.
    fn splice(&mut self, parent: usize, trace: &Value, program: &Value) {
        // Placed from the step that made the call rather than from the two
        // `started` times: those are whole milliseconds, and a guest's steps
        // are often a fraction of one. Its root began the moment it was
        // called, which is when that step began.
        let offset = self.steps.get(parent).map_or(0.0, |step| step.at);
        let base = self.steps.len();
        let steps = field(trace, "steps");
        let Value::Array(steps) = &steps else {
            return;
        };
        for step in steps.iter() {
            if self.steps.len() >= MOST_STEPS {
                self.dropped += 1;
                continue;
            }
            let own_parent = match field(step, "parent") {
                Value::Number(p) => Some(base + p as usize),
                _ => Some(parent),
            };
            self.steps.push(Step {
                parent: own_parent,
                target: text(&field(step, "target")),
                class: text(&field(step, "class")),
                at: offset + number(&field(step, "at")),
                ms: number(&field(step, "ms")),
                ok: matches!(field(step, "ok"), Value::Bool(true)),
                program: Some(match field(step, "program") {
                    Value::Null => program.clone(),
                    nested => nested,
                }),
            });
        }
        self.dropped += number(&field(trace, "dropped")) as usize;
    }
}

impl Interaction {
    fn into_particle(self) -> Value {
        let (ms, ok) = self
            .steps
            .first()
            .map(|root| (root.ms, root.ok))
            .unwrap_or((0.0, true));
        let root_class = crate::trace::class_of(&self.particle).to_string();
        let steps = self
            .steps
            .into_iter()
            .enumerate()
            .map(|(id, step)| {
                object(vec![
                    ("id", Value::Number(id as f64)),
                    (
                        "parent",
                        step.parent.map_or(Value::Null, |p| Value::Number(p as f64)),
                    ),
                    ("target", string(&step.target)),
                    ("class", string(&step.class)),
                    ("at", Value::Number(round(step.at))),
                    ("ms", Value::Number(round(step.ms))),
                    ("ok", Value::Bool(step.ok)),
                    ("program", step.program.unwrap_or(Value::Null)),
                ])
            })
            .collect();
        object(vec![
            ("_class", string(HANDLER)),
            (
                "root",
                object(vec![
                    ("from", string(&self.from)),
                    ("class", string(&root_class)),
                    ("particle", self.particle),
                    ("program", Value::Null),
                ]),
            ),
            ("started", Value::Number(self.started.floor())),
            ("ms", Value::Number(round(ms))),
            ("ok", Value::Bool(ok)),
            ("steps", Value::Array(Rc::new(steps))),
            ("dropped", Value::Number(self.dropped as f64)),
        ])
    }
}

/// A held program's own interaction, marked with which one it was: the
/// root and every step that does not already say.
fn stamp(trace: Value, program: &Value) -> Value {
    let Value::Object(fields) = &trace else {
        return trace;
    };
    let mark = |value: &Value| -> Value {
        let Value::Object(inner) = value else {
            return value.clone();
        };
        Value::Object(Rc::new(
            inner
                .iter()
                .map(|(k, v)| match (k.as_str(), v) {
                    ("program", Value::Null) => (k.clone(), program.clone()),
                    _ => (k.clone(), v.clone()),
                })
                .collect(),
        ))
    };
    Value::Object(Rc::new(
        fields
            .iter()
            .map(|(k, v)| match (k.as_str(), v) {
                ("root", _) => (k.clone(), mark(v)),
                ("steps", Value::Array(steps)) => (
                    k.clone(),
                    Value::Array(Rc::new(steps.iter().map(&mark).collect())),
                ),
                _ => (k.clone(), v.clone()),
            })
            .collect(),
    ))
}

fn field(value: &Value, name: &str) -> Value {
    match value {
        Value::Object(fields) => fields
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

fn number(value: &Value) -> f64 {
    match value {
        Value::Number(n) => *n,
        _ => 0.0,
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::Str(s) => s.to_string(),
        _ => String::new(),
    }
}

/// Whether an answer is an Exception — what makes a step not ok.
pub fn is_exception(answer: &Value) -> bool {
    crate::trace::class_of(answer) == "Exception"
}

/// Thousandths of a millisecond are noise in a trace and make every number
/// in it eighteen digits long.
fn round(ms: f64) -> f64 {
    (ms * 1000.0).round() / 1000.0
}

fn string(text: &str) -> Value {
    Value::Str(text.into())
}

fn object(fields: Vec<(&str, Value)>) -> Value {
    Value::Object(Rc::new(
        fields
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    ))
}

#[cfg(not(target_arch = "wasm32"))]
fn unix_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        * 1000.0
}

#[cfg(not(target_arch = "wasm32"))]
fn monotonic_ms() -> f64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

#[cfg(target_arch = "wasm32")]
fn unix_ms() -> f64 {
    js_sys::Date::now()
}

/// The browser build has no monotonic clock without a DOM dependency; the
/// wall clock is what a page has, and a trace there is a sketch anyway.
#[cfg(target_arch = "wasm32")]
fn monotonic_ms() -> f64 {
    js_sys::Date::now()
}

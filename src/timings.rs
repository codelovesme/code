//! `code build --timings` (or `CODE_TIMINGS=1`, which reaches a build that
//! another tool starts, such as `euglena build`): where a build's time went.
//!
//! A build is a few stages in a row — read and check the program, turn it
//! into LLVM IR, let LLVM's backend make machine code, compile the C
//! runtime, link — and which one dominates depends on the program: a small
//! one is mostly the runtime's compile, a large one mostly the backend. The
//! report names each stage and, because the backend's cost follows how much
//! IR each function hands it, the functions with the most IR instructions.
//!
//! Off, every call here is a read of one flag.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static ON: AtomicBool = AtomicBool::new(false);
static STAGES: Mutex<Vec<(&'static str, Duration)>> = Mutex::new(Vec::new());
static FUNCTIONS: Mutex<Vec<(String, usize)>> = Mutex::new(Vec::new());
static NOTES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// How many of the largest functions the report names.
const SHOWN: usize = 8;

/// Turns the report on: by the flag, or by `CODE_TIMINGS` set to anything
/// but empty or `0`.
pub fn enable(flag: bool) {
    let by_env = std::env::var("CODE_TIMINGS").is_ok_and(|v| !v.is_empty() && v != "0");
    if flag || by_env {
        ON.store(true, Ordering::Relaxed);
    }
}

pub fn enabled() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Adds `elapsed` to the stage `name` (a stage met twice adds up).
pub fn record(name: &'static str, elapsed: Duration) {
    if !enabled() {
        return;
    }
    let mut stages = STAGES.lock().unwrap_or_else(|e| e.into_inner());
    match stages.iter_mut().find(|(n, _)| *n == name) {
        Some((_, total)) => *total += elapsed,
        None => stages.push((name, elapsed)),
    }
}

/// Runs `f` as the stage `name`.
pub fn measure<T>(name: &'static str, f: impl FnOnce() -> T) -> T {
    if !enabled() {
        return f();
    }
    let start = Instant::now();
    let out = f();
    record(name, start.elapsed());
    out
}

/// A line the report adds under the stages (how many parts, how many came
/// from the cache).
pub fn note(line: String) {
    if enabled() {
        NOTES.lock().unwrap_or_else(|e| e.into_inner()).push(line);
    }
}

/// The IR size of each function the backend is about to compile.
pub fn functions(sizes: Vec<(String, usize)>) {
    if enabled() {
        *FUNCTIONS.lock().unwrap_or_else(|e| e.into_inner()) = sizes;
    }
}

/// The report, or `None` when timings are off. `total` is the build's wall
/// time; what no stage accounts for is shown as `other`, so the rows add up.
pub fn report(total: Duration) -> Option<String> {
    if !enabled() {
        return None;
    }
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let mut out = String::from("timings:\n");
    let stages = STAGES.lock().unwrap_or_else(|e| e.into_inner());
    let mut counted = Duration::ZERO;
    for (name, elapsed) in stages.iter() {
        counted += *elapsed;
        out.push_str(&format!("  {:>9.1} ms  {name}\n", ms(*elapsed)));
    }
    out.push_str(&format!(
        "  {:>9.1} ms  other\n",
        ms(total.saturating_sub(counted))
    ));
    out.push_str(&format!("  {:>9.1} ms  total\n", ms(total)));
    for line in NOTES.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        out.push_str(&format!("{line}\n"));
    }
    let mut functions = FUNCTIONS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let all: usize = functions.iter().map(|(_, n)| n).sum();
    if all > 0 {
        functions.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out.push_str(&format!(
            "IR: {all} instructions in {} functions; largest:\n",
            functions.len()
        ));
        for (name, n) in functions.iter().take(SHOWN) {
            out.push_str(&format!(
                "  {n:>9}  {:>5.1}%  {name}\n",
                100.0 * *n as f64 / all as f64
            ));
        }
    }
    Some(out)
}

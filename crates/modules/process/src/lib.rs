//! The `process` native module — run other programs, for the Code
//! programming language, written in Rust on [`code-native`].
//!
//! Two ways to run something:
//!
//! - `Run { command, args?, cwd?, env?, stdin? }` → `RunResult { code,
//!   success, stdout, stderr }` — run to completion, capture its output.
//!   Blocks the program until the child exits, the same way `http_client`
//!   blocks for a round trip. This is the one you want for `git`, `ffmpeg`,
//!   a build step.
//! - `Spawn { id, command, args?, cwd?, env? }` → `SpawnResult { id, pid }`
//!   — start a long-running child, tracked under a caller-chosen `id`, with
//!   its output inherited (it writes to this program's stdout/stderr). Then
//!   `Status { id }`, `Wait { id }`, `Kill { id }`, `List {}`.
//!
//! No configuration and no setup particle — the process table starts empty
//! and fills as you `Spawn`.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use code_native::*;
use std::collections::HashMap;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

/// Long-running children started by `Spawn`, keyed by the caller's `id`.
/// `Run` never touches this — it waits inline and returns.
static TRACKED: Mutex<Option<HashMap<String, Tracked>>> = Mutex::new(None);

struct Tracked {
    child: Child,
    command: String,
    /// `None` while running; `Some(code)` once reaped (`-1` = killed / no code).
    exit_code: Option<i64>,
}

/// The ABI version this module speaks. Must equal `CODE_ABI_VERSION` or the
/// host refuses to load us.
#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// The single dispatch point: read `_class`, route to a handler. An
/// unhandled class is null; a handler that cannot do the work returns an
/// `Exception`. Neither ends the program.
///
/// # Safety
///
/// Both pointers must be valid for reads/writes for the duration of the
/// call and laid out per `code_abi.h` — the host guarantees this.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "process", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Run" => run(out, particle),
            "Spawn" => spawn(out, particle),
            "Status" => status(out, particle),
            "Wait" | "WaitFor" => wait(out, particle),
            "Kill" => kill(out, particle),
            "List" => list(out),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "process", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Run — one-shot, output captured
// ---------------------------------------------------------------------------

fn run(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let mut cmd = build_command(particle)?;
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let stdin_text = find_field(particle, "stdin").and_then(read_str);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start '{}': {e}", command_name(particle)))?;

    if let Some(text) = stdin_text {
        if let Some(mut pipe) = child.stdin.take() {
            let _ = pipe.write_all(text.as_bytes());
        }
    }
    // Dropping any stdin we did not write closes it, so the child sees EOF.
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .map_err(|e| format!("waiting for the child failed: {e}"))?;
    let code = output.status.code().map(i64::from).unwrap_or(-1);

    let mut b = SlotBuffer::new(5);
    borrowed_str(b.slot_mut(0), c"RunResult");
    number(b.slot_mut(1), code as f64);
    boolean(b.slot_mut(2), output.status.success());
    owned_str(b.slot_mut(3), &String::from_utf8_lossy(&output.stdout));
    owned_str(b.slot_mut(4), &String::from_utf8_lossy(&output.stderr));
    object(
        out,
        &[c"_class", c"code", c"success", c"stdout", c"stderr"],
        &mut b,
    );
    b.release_all();
    Ok(())
}

// ---------------------------------------------------------------------------
// Spawn / Status / Wait / Kill / List — tracked children
// ---------------------------------------------------------------------------

fn spawn(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let id = require_str(particle, "id", "Spawn")?.to_string();
    let command = command_name(particle);
    let mut cmd = build_command(particle)?;
    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());

    let mut table = TRACKED.lock().unwrap_or_else(|e| e.into_inner());
    let table = table.get_or_insert_with(HashMap::new);

    if let Some(existing) = table.get_mut(&id) {
        reap(existing);
        if existing.exit_code.is_none() {
            return Err(format!("a process with id '{id}' is already running"));
        }
        table.remove(&id);
    }

    let child = cmd
        .spawn()
        .map_err(|e| format!("cannot start '{command}': {e}"))?;
    let pid = child.id();
    table.insert(
        id.clone(),
        Tracked {
            child,
            command,
            exit_code: None,
        },
    );

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"SpawnResult");
    owned_str(b.slot_mut(1), &id);
    number(b.slot_mut(2), pid as f64);
    object(out, &[c"_class", c"id", c"pid"], &mut b);
    b.release_all();
    Ok(())
}

fn status(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let id = require_str(particle, "id", "Status")?;
    let mut table = TRACKED.lock().unwrap_or_else(|e| e.into_inner());
    let entry = table
        .as_mut()
        .and_then(|t| t.get_mut(id))
        .ok_or_else(|| format!("no tracked process with id '{id}'"))?;
    reap(entry);
    status_result(out, id, entry);
    Ok(())
}

fn wait(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let id = require_str(particle, "id", "Wait")?;
    let mut table = TRACKED.lock().unwrap_or_else(|e| e.into_inner());
    let entry = table
        .as_mut()
        .and_then(|t| t.get_mut(id))
        .ok_or_else(|| format!("no tracked process with id '{id}'"))?;

    if entry.exit_code.is_none() {
        let code = entry
            .child
            .wait()
            .map(|s| s.code().map(i64::from).unwrap_or(-1))
            .map_err(|e| format!("waiting for '{id}' failed: {e}"))?;
        entry.exit_code = Some(code);
    }
    status_result(out, id, entry);
    Ok(())
}

fn kill(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let id = require_str(particle, "id", "Kill")?;
    let mut table = TRACKED.lock().unwrap_or_else(|e| e.into_inner());
    let entry = table
        .as_mut()
        .and_then(|t| t.get_mut(id))
        .ok_or_else(|| format!("no tracked process with id '{id}'"))?;
    reap(entry);
    let killed = if entry.exit_code.is_none() {
        let _ = entry.child.kill();
        let _ = entry.child.wait();
        entry.exit_code = Some(-1);
        true
    } else {
        false
    };

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"KillResult");
    owned_str(b.slot_mut(1), id);
    boolean(b.slot_mut(2), killed);
    object(out, &[c"_class", c"id", c"killed"], &mut b);
    b.release_all();
    Ok(())
}

fn list(out: &mut CodeValue) -> Result<(), String> {
    let mut table = TRACKED.lock().unwrap_or_else(|e| e.into_inner());
    let mut rows: Vec<(String, String, Option<i64>)> = Vec::new();
    if let Some(t) = table.as_mut() {
        for (id, entry) in t.iter_mut() {
            reap(entry);
            rows.push((id.clone(), entry.command.clone(), entry.exit_code));
        }
    }
    rows.sort();

    let mut arr = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(rows.len());
    for (i, (id, command, code)) in rows.iter().enumerate() {
        let mut e = SlotBuffer::new(5);
        borrowed_str(e.slot_mut(0), c"ProcessEntry");
        owned_str(e.slot_mut(1), id);
        owned_str(e.slot_mut(2), command);
        boolean(e.slot_mut(3), code.is_none());
        owned_str(e.slot_mut(4), state_word(*code));
        object(
            buf.slot_mut(i as i64),
            &[c"_class", c"id", c"command", c"alive", c"status"],
            &mut e,
        );
        e.release_all();
    }
    array(&mut arr, &mut buf);
    buf.release_all();

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"ProcessList");
    copy(b.slot_mut(1), &arr);
    number(b.slot_mut(2), rows.len() as f64);
    object(out, &[c"_class", c"processes", c"count"], &mut b);
    b.release_all();
    release(&mut arr);
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

/// Poll a tracked child without blocking; record its exit code if it has
/// finished. A no-op once already reaped.
fn reap(entry: &mut Tracked) {
    if entry.exit_code.is_some() {
        return;
    }
    match entry.child.try_wait() {
        Ok(Some(s)) => entry.exit_code = Some(s.code().map(i64::from).unwrap_or(-1)),
        Ok(None) => {}
        Err(_) => entry.exit_code = Some(-1),
    }
}

fn state_word(code: Option<i64>) -> &'static str {
    match code {
        None => "running",
        Some(0) => "exited",
        Some(_) => "failed",
    }
}

fn status_result(out: &mut CodeValue, id: &str, entry: &Tracked) {
    let mut b = SlotBuffer::new(5);
    borrowed_str(b.slot_mut(0), c"StatusResult");
    owned_str(b.slot_mut(1), id);
    boolean(b.slot_mut(2), entry.exit_code.is_none());
    number(b.slot_mut(3), entry.exit_code.unwrap_or(-1) as f64);
    owned_str(b.slot_mut(4), state_word(entry.exit_code));
    object(
        out,
        &[c"_class", c"id", c"alive", c"code", c"status"],
        &mut b,
    );
    b.release_all();
}

/// A `Command` from `command` + `args?` + `cwd?` + `env?`, validated. Does
/// not set stdio — the caller ([`run`] pipes, [`spawn`] inherits).
fn build_command(particle: &CodeValue) -> Result<Command, String> {
    let name = require_str(particle, "command", "process")?;
    let mut cmd = Command::new(name);

    if let Some(args) = find_field(particle, "args") {
        if args.tag != CodeTag::Array {
            return Err("'args' must be an array of strings".to_string());
        }
        for arg in array_elems(args) {
            cmd.arg(read_str(arg).ok_or("every 'args' element must be a string")?);
        }
    }
    if let Some(cwd) = find_field(particle, "cwd").and_then(read_str) {
        cmd.current_dir(cwd);
    }
    if let Some(env) = find_field(particle, "env") {
        if env.tag != CodeTag::Object {
            return Err("'env' must be an object of string values".to_string());
        }
        // Start from an empty environment only if asked; otherwise add to the
        // inherited one, which is what a caller passing two overrides expects.
        for (key, value) in object_entries(env) {
            cmd.env(key, read_str(value).ok_or("every 'env' value must be a string")?);
        }
    }
    Ok(cmd)
}

fn command_name(particle: &CodeValue) -> String {
    find_field(particle, "command")
        .and_then(read_str)
        .unwrap_or("")
        .to_string()
}

fn require_str<'a>(particle: &'a CodeValue, name: &str, class: &str) -> Result<&'a str, String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{class} requires a non-empty string '{name}'"))
}

//! A host that defines `Trace` sees inside the programs it holds: a call
//! into one is spliced into the host's own trace, under the step that made
//! it, and what a held program does on its own — a module of its own pushing
//! — is an interaction of its own, handed to the host's `Trace` too. See
//! `docs/todo/trace-handler.md`, stage C.
//!
//! A Rust test rather than a fixture because the guest has to be *built*:
//! a `.so` compiled from `.code`, holding a module of its own.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

#[path = "support/modules.rs"]
mod modules;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const GUEST: &str = r#"link "test_events.so" as ev

Shout { text } =>
    return Shouted { text = text }

Ping { who } =>
    emit Shout { text = who } to this get loud
    return Pong { text = loud.text }

Kick {} =>
    emit Start { value = 2 } to ev
    return Kicked

Tick { value } =>
    emit Shout { text = "tick" } to this
    return Ack
"#;

const HOST: &str = r#"traces = []

Trace { root, ok, steps } =>
    traces += [{ root = root, ok = ok, steps = steps }]

Hold {} =>
    link "guest.so" as app
    emit Ping { who = "ada" } to app get pong
    emit Kick {} to app
    return Held { text = pong.text }

emit Hold {} to this get held
assert held.text = "ada"

| One interaction of the host's, then the two ticks the guest's own module
| pushed, each an interaction of the guest's.
emit Length { value = traces } to core get count
assert count.value = 3

hold = traces[0]
assert hold.root.class = "Hold"
assert hold.root.program = null
steps = hold.steps
emit Length { value = steps } to core get stepped
assert stepped.value = 7

| The host's call into the guest...
assert steps[1].class = "Ping"
assert steps[1].target = "module:app"
assert steps[1].parent = 0
assert steps[1].program = null
| ...and what it did inside, under that call.
assert steps[2].class = "Ping"
assert steps[2].target = "this"
assert steps[2].parent = 1
assert steps[2].at ≥ steps[1].at
assert steps[2].program = "./guest.so"
assert steps[3].class = "Shout"
assert steps[3].parent = 2
assert steps[3].program = "./guest.so"
| The second call, and the guest reaching a module of its own.
assert steps[4].class = "Kick"
assert steps[4].parent = 0
assert steps[5].parent = 4
assert steps[6].class = "Start"
assert steps[6].target = "module:ev"
assert steps[6].parent = 5
assert steps[6].program = "./guest.so"
assert steps[6].at ≥ steps[5].at

tick = traces[1]
assert tick.root.from = "ev"
assert tick.root.class = "Tick"
assert tick.root.program = "./guest.so"
assert tick.steps[0].program = "./guest.so"
assert tick.steps[1].class = "Shout"
assert tick.steps[1].parent = 0
assert traces[2].root.particle.value = 1
"#;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("code-trace-hosted-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    dir
}

fn setup(dir: &Path) {
    let built = modules::build_so("test_events");
    fs::copy(&built, dir.join("test_events.so")).expect("place test_events");
    let guest = dir.join("guest.code");
    fs::write(&guest, GUEST).expect("write guest");
    code::compile_file(
        &guest,
        code::BuildTarget::Shared,
        &dir.join("guest.so"),
        false,
    )
    .expect("build the guest");
    fs::write(dir.join("main.code"), HOST).expect("write host");
}

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn an_interpreted_host_sees_inside_what_it_holds() {
    let dir = temp_dir("interp");
    setup(&dir);
    let run = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("run")
        .arg("main.code")
        .current_dir(&dir)
        .env("CODE_CHECK_LEAKS", "1")
        .output()
        .expect("spawn code run");
    assert!(
        run.status.success(),
        "interpreted host:\n{}",
        stderr_of(&run)
    );
}

#[test]
fn a_compiled_host_sees_inside_what_it_holds() {
    let dir = temp_dir("compiled");
    setup(&dir);
    let exe = dir.join("main");
    code::compile_file(&dir.join("main.code"), code::BuildTarget::Exe, &exe, false)
        .expect("compile the host");
    let run = Command::new(&exe)
        .current_dir(&dir)
        .env("CODE_CHECK_LEAKS", "1")
        .output()
        .expect("run the compiled host");
    assert!(run.status.success(), "compiled host:\n{}", stderr_of(&run));
}

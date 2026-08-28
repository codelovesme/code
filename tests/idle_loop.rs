//! An empty `loop { }` waits; it does not spin.
//!
//! `loop { }` is how a program says "keep me up" — the owner's call, from
//! `docs/todo/inbound-emissions-from-native-modules.md`. A program shaped
//! like that has nothing to do but wait for what its modules push, and it
//! never ends: there is no statement in the body to reach a `break`. So the
//! only thing left to get wrong is how much of the machine it costs while it
//! waits, and the difference between right and wrong is not visible in
//! anything a `.code` fixture can assert — both versions produce the same
//! (absence of) output, forever.
//!
//! Hence a test that watches the process instead of its output: run it for
//! half a second and read the CPU time the kernel charged it. Sleeping a
//! millisecond a time round costs ~0; spinning costs a whole core. The two
//! are ~50x apart (measured: 0-1 ticks against 49), so the threshold is not
//! a close call, and no timing assumption beyond "500ms of wall clock is not
//! 100ms of CPU" is being made.
//!
//! Linux-only, because `/proc/<pid>/stat` is where a child's CPU time is
//! readable without waiting for it to exit — and this child never exits.
//! Skipped elsewhere rather than failed.

#![cfg(all(feature = "llvm", target_os = "linux"))]

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// CPU time, in clock ticks (100/s on every Linux worth naming), that a
/// spinning half-second would blow straight through and a sleeping one never
/// approaches.
const MAX_TICKS: u64 = 10;
const OBSERVE: Duration = Duration::from_millis(500);

#[test]
fn an_empty_loop_costs_nothing_while_it_waits() {
    let dir = std::env::temp_dir().join(format!("code-idle-loop-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    let source = dir.join("idle.code");
    fs::write(&source, "loop {\n}\n").expect("write the fixture");

    let interpreted = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("run")
        .arg(&source)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn code run");
    assert_idle("code run", interpreted);

    let binary = dir.join("idle");
    code::compile_file(&source, code::BuildTarget::Exe, &binary, false).expect("code build");
    let compiled = Command::new(&binary)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the compiled program");
    assert_idle("code build", compiled);

    let _ = fs::remove_dir_all(&dir);
}

/// Lets `child` run for `OBSERVE`, then reads what it spent and kills it.
/// The kill is unconditional — the process is an intentional daemon, so
/// leaving it behind on a failure would leak a spinning core into every
/// later test in this run.
fn assert_idle(mode: &str, mut child: Child) {
    std::thread::sleep(OBSERVE);
    let spent = cpu_ticks(child.id());
    let alive = child.try_wait().expect("poll the child").is_none();
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        alive,
        "{mode}: the program ended on its own — an empty `loop {{ }}` has no way out, \
         so something failed before the loop was reached"
    );
    let spent = spent.unwrap_or_else(|| panic!("{mode}: could not read the child's CPU time"));
    assert!(
        spent <= MAX_TICKS,
        "{mode}: spent {spent} ticks of CPU in {}ms of waiting (limit {MAX_TICKS}) — \
         an empty `loop {{ }}` is spinning instead of sleeping",
        OBSERVE.as_millis()
    );
}

/// utime + stime for a live process, from `/proc/<pid>/stat` — fields 14 and
/// 15, counted from 1. The comm field (2) can itself contain spaces and
/// parentheses, so everything up to the last `)` is skipped rather than
/// split on.
fn cpu_ticks(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(PathBuf::from(format!("/proc/{pid}/stat"))).ok()?;
    let after_comm = &stat[stat.rfind(')')? + 1..];
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    // `after_comm` starts at field 3, so 14 and 15 are offsets 11 and 12.
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some(utime + stime)
}

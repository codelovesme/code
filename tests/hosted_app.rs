//! A program holding *other programs* in memory: linking them while it runs,
//! talking to each one separately, and stopping them so their memory comes
//! back.
//!
//! The guest here is an ordinary application. Nothing in its source says it
//! is a guest, and the same file is built twice — once as a `.so` a host
//! links, once as a program that runs on its own — so "hosting changed
//! nothing about it" is a comparison rather than a claim.
//!
//! `tests/runtime_link_basic.code` and `tests/runtime_link_failures.code`
//! cover the statement itself against hand-written modules, in both output
//! modes, and are the cheaper place to add a case. What needs a Rust test is
//! only what needs a *build*: a guest compiled from `.code`, and the leak
//! check that proves stopping it gave the memory back.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// An application that owns heap blocks — `greeting` and `history` both do,
/// and that is the point rather than decoration. A guest whose whole world
/// is literals owns nothing, so releasing it would prove nothing: the leak
/// check below can only see blocks that were really allocated.
const GUEST_SOURCE: &str = r#"let greeting = "hello " + ""
let history = [1, 2, 3]
export let name = "gu" + "est"

Ping { who } => {
    return Pong { text = greeting + who, seen = history }
}
"#;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("code-hosted-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    dir
}

fn build(dir: &Path, name: &str, source: &str, target: code::BuildTarget, out: &str) -> PathBuf {
    let path = dir.join(format!("{name}.code"));
    fs::write(&path, source).expect("write source");
    let artifact = dir.join(out);
    code::compile_file(&path, target, &artifact, false)
        .unwrap_or_else(|e| panic!("build {name} for {target:?}: {e}"));
    artifact
}

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Runs `dir/main.code` both ways with the leak check on, and requires both
/// to succeed. The program asserts for itself, so success is the whole
/// result — and running it in both modes is what keeps the two backends'
/// answers from drifting apart.
fn run_both_ways(dir: &Path, what: &str) {
    let interpreted = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("run")
        .arg("main.code")
        .current_dir(dir)
        .env("CODE_CHECK_LEAKS", "1")
        .output()
        .expect("spawn code run");
    assert!(
        interpreted.status.success(),
        "interpreted: {what}\n{}",
        stderr_of(&interpreted)
    );

    let exe = dir.join("main");
    code::compile_file(&dir.join("main.code"), code::BuildTarget::Exe, &exe, false)
        .expect("compile the host");
    let compiled = Command::new(&exe)
        .current_dir(dir)
        .env("CODE_CHECK_LEAKS", "1")
        .output()
        .expect("run the compiled host");
    assert!(
        compiled.status.success(),
        "compiled: {what}\n{}",
        stderr_of(&compiled)
    );
}

/// The same application, hosted and standalone, answers the same thing.
///
/// This is the constraint the whole design exists to meet: an application
/// must not have to know where it is running. The standalone half runs the
/// guest's own source as a program that asks itself; the hosted half never
/// touches that source again, only the `.so` built from it.
#[test]
fn a_guest_answers_the_same_hosted_or_alone() {
    let dir = temp_dir("same");
    build(
        &dir,
        "guest",
        GUEST_SOURCE,
        code::BuildTarget::Shared,
        "guest.so",
    );

    // Standalone: the guest's own source, plus the question, run as one
    // ordinary program.
    let alone = dir.join("alone.code");
    fs::write(
        &alone,
        format!("{GUEST_SOURCE}\nemit Ping {{ who = \"world\" }} to this get r\nassert r.text = \"hello world\"\nassert r.seen = [1, 2, 3]\n"),
    )
    .expect("write standalone");
    let out = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("run")
        .arg("alone.code")
        .current_dir(&dir)
        .output()
        .expect("spawn code run");
    assert!(
        out.status.success(),
        "the guest does not pass its own test standalone:\n{}",
        stderr_of(&out)
    );

    // Hosted: the same expectations, asked through a host that linked the
    // `.so` while it was running.
    fs::write(
        dir.join("main.code"),
        r#"Ask { path } => {
    link path as app
    emit Ping { who = "world" } to app get r
    unlink app
    return r
}

emit Ask { path = "guest.so" } to this get r
assert r.text = "hello world"
assert r.seen = [1, 2, 3]
"#,
    )
    .expect("write host");
    run_both_ways(
        &dir,
        "a hosted guest answered differently than it does alone",
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Stopping an application gives its memory back.
///
/// `CODE_CHECK_LEAKS=1` is what makes this a check. A guest carries its own
/// copy of the runtime with its own block counter, and `unlink` runs the
/// guest's release point — which ends in the guest's *own* `code_check_leaks`
/// — before unloading it. So a guest that still owned anything would end the
/// run right there, naming the blocks. Both output modes, because the two
/// backends reach that release point by different routes.
#[test]
fn stopping_a_guest_reclaims_its_memory() {
    let dir = temp_dir("memory");
    build(
        &dir,
        "guest",
        GUEST_SOURCE,
        code::BuildTarget::Shared,
        "guest.so",
    );
    fs::write(
        dir.join("main.code"),
        r#"| Started and stopped repeatedly: if stopping leaked, doing it many
| times is where it would show.
Cycle { path } => {
    link path as app
    emit Ping { who = "x" } to app get r
    unlink app
    return r
}

loop i over [1, 2, 3, 4, 5] {
    emit Cycle { path = "guest.so" } to this get r
    assert r.text = "hello x"
}
"#,
    )
    .expect("write host");
    run_both_ways(&dir, "a stopped guest did not give its memory back");

    let _ = fs::remove_dir_all(&dir);
}

/// Two applications at once, each its own address, each stopped on its own.
///
/// Two *files*, deliberately: the dynamic loader hands back the same mapping
/// for a path already open, so linking one file twice is one organelle under
/// two names (which `tests/runtime_link_basic.code` covers). Separate guests
/// are separate organelles, and this is where "the host talks to each one
/// separately" is actually tested.
#[test]
fn two_guests_are_held_and_stopped_independently() {
    let dir = temp_dir("two");
    build(
        &dir,
        "guest_a",
        GUEST_SOURCE,
        code::BuildTarget::Shared,
        "a.so",
    );
    build(
        &dir,
        "guest_b",
        GUEST_SOURCE,
        code::BuildTarget::Shared,
        "b.so",
    );
    fs::write(
        dir.join("main.code"),
        r#"let a = null
let b = null

Start { } => {
    link "a.so" as one
    a = one
    link "b.so" as two
    b = two
    return Started { }
}

AskA { who } => {
    emit Ping { who = who } to a get r
    return r
}

AskB { who } => {
    emit Ping { who = who } to b get r
    return r
}

StopA { } => {
    unlink a
    return Stopped { }
}

emit Start { } to this get s
assert s._class = "Started"

emit AskA { who = "one" } to this get ra
assert ra.text = "hello one"
emit AskB { who = "two" } to this get rb
assert rb.text = "hello two"

| Separate files, so separate organelles: the two addresses are not equal.
assert not a = b

emit StopA { } to this get stopped
assert stopped._class = "Stopped"

| Stopping one leaves the other exactly as it was.
emit AskB { who = "still" } to this get alive
assert alive.text = "hello still"

| And the stopped one is stopped.
emit AskA { who = "gone" } to this get after
assert after._class = "Exception"
"#,
    )
    .expect("write host");
    run_both_ways(&dir, "two guests did not stay independent");

    let _ = fs::remove_dir_all(&dir);
}

/// A guest left running when the host ends still reaches its release point.
///
/// The host never says `unlink`. Under `CODE_CHECK_LEAKS=1` the guest's own
/// counter is checked from inside that release point, so if the end-of-program
/// sweep skipped it the run would fail here — which is exactly what it did
/// before the sweep existed.
#[test]
fn a_guest_still_linked_at_exit_is_released_anyway() {
    let dir = temp_dir("exit");
    build(
        &dir,
        "guest",
        GUEST_SOURCE,
        code::BuildTarget::Shared,
        "guest.so",
    );
    fs::write(
        dir.join("main.code"),
        r#"let app = null

Start { } => {
    link "guest.so" as a
    app = a
    return Started { }
}

emit Start { } to this get s
assert s._class = "Started"
emit Ping { who = "x" } to app get r
assert r.text = "hello x"
"#,
    )
    .expect("write host");
    run_both_ways(&dir, "a guest still linked at exit was never released");

    let _ = fs::remove_dir_all(&dir);
}

/// A guest gets the *host's* organelles, not its own.
///
/// This is the constraint the whole design exists for. The guest links an
/// organelle by name and uses it, exactly as it would running alone — and
/// what it reaches is whatever the host chose to put there. The numbers are
/// picked so the two are impossible to confuse: the real module doubles, the
/// host's stand-in multiplies by ten.
#[test]
fn a_guest_reaches_the_hosts_organelles_not_its_own() {
    let dir = temp_dir("offer");
    fs::create_dir_all(dir.join("native_modules")).expect("create module dir");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/native_modules/test_math.so"),
        dir.join("native_modules/test_math.so"),
    )
    .expect("copy the real module next to the guest");

    const GUEST: &str = r#"link "native_modules/test_math.so" as m

Work { value } => {
    emit Double { value = value } to m get d
    return Done { value = d.value }
}
"#;
    build(&dir, "guest", GUEST, code::BuildTarget::Shared, "guest.so");

    // Alone, the guest reaches the real module: 3 doubled is 6.
    fs::write(
        dir.join("alone.code"),
        format!("{GUEST}\nemit Work {{ value = 3 }} to this get r\nassert r.value = 6\n"),
    )
    .expect("write standalone");
    let out = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("run")
        .arg("alone.code")
        .current_dir(&dir)
        .output()
        .expect("spawn code run");
    assert!(
        out.status.success(),
        "the guest does not reach the real module when alone:\n{}",
        stderr_of(&out)
    );

    // Hosted, the same source reaches the host's stand-in instead: 3 becomes
    // 30. The guest's own line is unchanged and it never learns the
    // difference.
    fs::write(
        dir.join("main.code"),
        r#"Offer { app, name } => {
    if name = "test_math" { return Offered { } }
    return Denied { }
}

Organelle { app, name, particle } => {
    if particle._class = "Double" { return DoubleResult { value = particle.value * 10 } }
    return Denied { }
}

Run { } => {
    link "./guest.so" as app
    emit Work { value = 3 } to app get r
    unlink app
    return r
}

emit Run { } to this get r
assert r.value = 30
"#,
    )
    .expect("write host");
    run_both_ways(
        &dir,
        "a hosted guest reached its own organelle, not the host's",
    );

    let _ = fs::remove_dir_all(&dir);
}

/// A host that refuses an organelle survives refusing it.
///
/// The obvious reading of "strict" would be to fail the guest's `link`. That
/// cannot be it: a guest's top-level `link` failing ends the guest, and a
/// fatal error inside a module ends the process it was loaded into — so a
/// host would be killed by its own policy, by a guest it deliberately said no
/// to. Measured, before this behaved otherwise.
///
/// So a refused organelle is one that refuses. The guest links it and finds
/// out on first use, the way it would find out about an unreachable network.
#[test]
fn a_refused_organelle_refuses_rather_than_ending_the_host() {
    let dir = temp_dir("refused");
    fs::create_dir_all(dir.join("native_modules")).expect("create module dir");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/native_modules/test_math.so"),
        dir.join("native_modules/test_math.so"),
    )
    .expect("copy a module the host will refuse");

    build(
        &dir,
        "guest",
        r#"link "native_modules/test_math.so" as m

Work { value } => {
    emit Double { value = value } to m get d
    return Done { answer = d._class, message = d.message }
}
"#,
        code::BuildTarget::Shared,
        "guest.so",
    );
    fs::write(
        dir.join("main.code"),
        r#"| Offers nothing at all.
Offer { app, name } => {
    return Denied { }
}

Run { } => {
    link "./guest.so" as app
    emit Work { value = 3 } to app get r
    unlink app
    return r
}

| The host is still here to ask, which is the point.
emit Run { } to this get r
assert r._class = "Done"
assert r.answer = "Exception"
assert r.message = "organelle 'test_math' is not offered by the host"
"#,
    )
    .expect("write host");
    run_both_ways(&dir, "a refused organelle did not refuse cleanly");

    let _ = fs::remove_dir_all(&dir);
}

/// A host may answer differently for different guests, and one guest stopped
/// does not disturb the next one started.
///
/// The `app` field is what makes the first half possible: it says which guest
/// is asking, so telling them apart needs no machinery beyond an `if`. The
/// second half is the case that took longest to get right — a guest opened
/// after another was stopped used to inherit the stopped one's world, because
/// a field added to the module handle was left uninitialised on one of its
/// three construction paths.
#[test]
fn a_host_may_offer_one_guest_what_it_denies_another() {
    let dir = temp_dir("perguest");
    fs::create_dir_all(dir.join("native_modules")).expect("create module dir");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/native_modules/test_math.so"),
        dir.join("native_modules/test_math.so"),
    )
    .expect("copy the module");

    const ASKS: &str = r#"link "native_modules/test_math.so" as m

Work { value } => {
    emit Double { value = value } to m get d
    return Done { answer = d._class }
}
"#;
    build(
        &dir,
        "trusted",
        ASKS,
        code::BuildTarget::Shared,
        "trusted.so",
    );
    build(&dir, "other", ASKS, code::BuildTarget::Shared, "other.so");

    fs::write(
        dir.join("main.code"),
        r#"Offer { app, name } => {
    if app = "./trusted.so" { return Offered { } }
    return Denied { }
}

Organelle { app, name, particle } => {
    return DoubleResult { value = 1 }
}

Ask { path } => {
    link path as a
    emit Work { value = 3 } to a get r
    unlink a
    return r
}

emit Ask { path = "./trusted.so" } to this get yes
assert yes.answer = "DoubleResult"

| Started after the first was stopped, and answered for itself.
emit Ask { path = "./other.so" } to this get no
assert no.answer = "Exception"
"#,
    )
    .expect("write host");
    run_both_ways(&dir, "the host could not tell its guests apart");

    let _ = fs::remove_dir_all(&dir);
}

/// The name a host matches on is the organelle's, not the file's.
///
/// A guest is compiled against whatever path its module resolver produced,
/// and in a real project that is a versioned, platform-suffixed asset deep
/// under `.code/modules/`. A host handler saying `if name = "test_math"` has
/// to keep working across all of it — otherwise every host would be matching
/// on someone else's deployment layout.
///
/// Found by hosting a real application: it asked for
/// `net_server-linux-x86_64` and was refused by a host that offers
/// `net_server`.
#[test]
fn a_host_sees_an_organelle_by_name_whatever_path_the_guest_carries() {
    let dir = temp_dir("naming");
    let nested = dir.join(".code/modules/test_math/9.9.9");
    fs::create_dir_all(&nested).expect("create module dir");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/native_modules/test_math.so"),
        nested.join("test_math-linux-x86_64.so"),
    )
    .expect("copy the module under a release-shaped name");

    build(
        &dir,
        "guest",
        r#"link ".code/modules/test_math/9.9.9/test_math-linux-x86_64.so" as m

Work { value } => {
    emit Double { value = value } to m get d
    return Done { value = d.value }
}
"#,
        code::BuildTarget::Shared,
        "guest.so",
    );
    fs::write(
        dir.join("main.code"),
        r#"| Matches the plain name, and never sees the version or the platform.
Offer { app, name } => {
    if name = "test_math" { return Offered { } }
    return Denied { }
}

Organelle { app, name, particle } => {
    if name = "test_math" { return DoubleResult { value = particle.value * 10 } }
    return Denied { }
}

Run { } => {
    link "./guest.so" as app
    emit Work { value = 3 } to app get r
    unlink app
    return r
}

emit Run { } to this get r
assert r.value = 30
"#,
    )
    .expect("write host");
    run_both_ways(&dir, "the host did not recognise the organelle by name");

    let _ = fs::remove_dir_all(&dir);
}

/// A guest owns its own organelles, and hears what they say.
///
/// Two things at once, and they are the same thing. The host installs itself
/// on every guest, but a host with no `Offer` handler furnishes nothing — so
/// the guest opens its own organelle, its own file, its own settings, just as
/// it would running alone.
///
/// And then it hears it. An organelle may speak without being asked, into a
/// queue that a *program's* loop empties — but a guest is a library whose
/// stream ran once and returned, so nothing of its own would ever empty it.
/// Its pushes ring the host's bell instead, and the host's own drain hands
/// the guest its turn. One loop, no polling, and nothing at all while
/// everyone is idle.
///
/// A refused port is the cheapest way to make an organelle speak: no network
/// is needed, and `http_client` reports the refusal as an `Exception` it
/// pushes rather than as the answer it returns.
#[test]
fn a_guest_owns_its_organelles_and_hears_them() {
    let dir = temp_dir("owned");
    fs::create_dir_all(dir.join("native_modules")).expect("create module dir");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/native_modules/http_client.so"),
        dir.join("native_modules/http_client.so"),
    )
    .expect("copy the organelle the guest will open for itself");

    build(
        &dir,
        "guest",
        r#"link "native_modules/http_client.so" as web

export let heard = false

Work { } => {
    emit Get { url = "http://127.0.0.1:1/" } to web get r
    return Done { ok = r.ok }
}

Heard { } => {
    return Answer { heard = heard }
}

Exception { source, message } => {
    heard = true
}
"#,
        code::BuildTarget::Shared,
        "guest.so",
    );
    fs::write(
        dir.join("main.code"),
        r#"| A host that offers nothing: no `Offer` handler at all, so the guest
| below opens its own organelle rather than being furnished one.
let app = null

Start { } => {
    link "./guest.so" as a
    app = a
    return Started { }
}

Work { } => {
    emit Work { } to app get done
    return done
}

Ask { } => {
    emit Heard { } to app get a
    return a
}

emit Start { } to this get s
assert s._class = "Started"

| Its own organelle answered — a refused connection, not a refusal to lend.
emit Work { } to this get done
assert not done.ok

| And between these two statements this program drained, guests included.
emit Ask { } to this get answer
assert answer.heard
"#,
    )
    .expect("write host");
    run_both_ways(
        &dir,
        "a guest did not own or did not hear its own organelle",
    );

    let _ = fs::remove_dir_all(&dir);
}

/// An application that is still working cannot be unloaded, and says so.
///
/// Unmapping code a thread is running in is not a risk to weigh, it is a
/// crash. So `unlink` asks first — and the question is `code_abi.h` item 8,
/// the same one that keeps a program alive past its last statement, asked of
/// a held application rather than of an organelle. Its answer is computed
/// from the organelles it holds, exactly as a program computes its own.
///
/// It refuses rather than skipping silently, and that matters: told nothing,
/// a host would mark something stopped that is still answering on its own
/// port.
///
/// Then the other half. Only the application knows what it opened, so it is
/// told to close and does it itself — the host never touches an organelle it
/// did not lend. And stopping a door is not instantaneous: `Stop` asks, and
/// the accepting thread turns its own answer to no as its *last act*, after
/// its loop has exited. So this asserts that the application becomes
/// unloadable, not that it is unloadable the same instant.
#[test]
fn an_application_that_is_still_working_is_not_unloaded() {
    let dir = temp_dir("working");
    fs::create_dir_all(dir.join("native_modules")).expect("create module dir");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/native_modules/net_server.so"),
        dir.join("native_modules/net_server.so"),
    )
    .expect("copy the door the application will open for itself");

    build(
        &dir,
        "guest",
        r#"| An application that owns a real door, and knows how to shut it.
link "native_modules/net_server.so" as door

Impulse { particle } => {
    return Pong { }
}

| What a host says when it is stopping this application. Only this
| application knows what it opened, so only it can close it.
Closing { } => {
    emit Stop { } to door get s
    return Closed { ok = s.ok }
}

emit Config { port = 0 } to door get c
emit Listen { } to door get l
assert l.ok
"#,
        code::BuildTarget::Shared,
        "guest.so",
    );
    fs::write(
        dir.join("main.code"),
        r#"let app = null

Start { } => {
    link "./guest.so" as a
    app = a
    emit Wake { } to a get _
    return Started { }
}

TryStop { } => {
    unlink app
    return Stopped { }
}

Close { } => {
    emit Closing { } to app get c
    return c
}

emit Start { } to this get s
assert s._class = "Started"

| Its door is open, so unloading it is refused — and answered, not ignored.
emit TryStop { } to this get refused
assert refused._class = "Exception"

| Told to close, it closes what it opened.
emit Close { } to this get closed
assert closed.ok

| And then it becomes unloadable. Not at once: the accepting thread turns
| its own answer to no as its last act, so this waits for that to happen
| rather than assuming it already has.
let done = null
loop attempt over [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] {
    let spins = 0
    loop {
        spins = spins + 1
        if spins > 300000 {
            break
        }
    }
    emit TryStop { } to this get answer
    done = answer
    if answer._class = "Stopped" {
        break
    }
}
assert done._class = "Stopped"
"#,
    )
    .expect("write host");
    run_both_ways(
        &dir,
        "an application was unloaded while still working, or never became stoppable",
    );

    let _ = fs::remove_dir_all(&dir);
}

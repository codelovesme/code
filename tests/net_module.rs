//! The `net_server` / `net_client` pair, driven by each other.
//!
//! A `.code` fixture cannot test this: it takes two programs, one still
//! running while the other sends to it. The no-network halves — every way
//! `Send` refuses, the whole `Config`/`Listen`/`Stop` lifecycle — live in
//! `tests/net_client_diagnostics.code` and `tests/net_server_lifecycle.code`
//! instead, where they cost nothing. What is here is the part that needs two
//! processes: a particle crossing the wire and an answer coming back.
//!
//! Both output modes, because a module is exactly where `code run` and
//! `code build` could differ.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use std::{fs, thread};

/// Build one module and return its `.so`.
fn build_module(name: &str) -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/modules")
        .join(name);
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        .status()
        .unwrap_or_else(|e| panic!("run cargo for {name}: {e}"));
    assert!(status.success(), "cargo failed to build {name}");
    crate_dir.join(format!("target/release/lib{name}.so"))
}

/// A port nothing is listening on: bind zero, read what the OS chose, let go.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind loopback")
        .local_addr()
        .expect("read bound port")
        .port()
}

/// Seconds of CPU (user + system) a process has used so far, from
/// `/proc/<pid>/stat`. Its second field is the executable name in parentheses
/// and may contain spaces, so the fields are counted from after the last `)`,
/// where the state (field 3) begins — making utime (14) and stime (15) the
/// twelfth and thirteenth from there. Linux-only, like everything else here.
fn cpu_seconds(pid: u32) -> f64 {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).expect("read /proc/<pid>/stat");
    let tail = &stat[stat.rfind(')').expect("the comm field is parenthesised") + 1..];
    let fields: Vec<&str> = tail.split_whitespace().collect();
    let ticks = |i: usize| fields[i].parse::<f64>().expect("a clock-tick count");
    (ticks(11) + ticks(12)) / 100.0
}

/// A directory with both modules in it, ready for `link "<name>.so"`.
fn workspace(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("code-net-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    // Beside the programs: `link` resolves against the linking file's own
    // directory first, so this needs no install and no search path.
    fs::copy(build_module("net_server"), dir.join("net_server.so")).expect("copy net_server.so");
    fs::copy(build_module("net_client"), dir.join("net_client.so")).expect("copy net_client.so");
    dir
}

/// Start `source` under `mode` (`"run"` interprets, `"build"` compiles first).
fn start(dir: &Path, mode: &str, source: &Path, exe_name: &str) -> Child {
    if mode == "run" {
        Command::new(env!("CARGO_BIN_EXE_code"))
            .arg("run")
            .arg(source)
            .current_dir(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn code run")
    } else {
        let exe = dir.join(exe_name);
        code::compile_file(source, code::BuildTarget::Exe, &exe, false).expect("compile");
        Command::new(&exe)
            .current_dir(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the compiled program")
    }
}

/// Run `source` to completion under `mode` and return whether it succeeded.
/// The client programs assert on what came back, so their exit status *is*
/// the assertion.
fn run_to_end(dir: &Path, mode: &str, source: &Path, exe_name: &str) -> bool {
    let mut child = start(dir, mode, source, exe_name);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait().expect("poll the client") {
            Some(status) => return status.success(),
            None if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{mode}: the client never finished");
            }
        }
    }
}

/// Wait until something is listening on `port`, so a client never races the
/// server's `Listen`.
fn await_listening(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("nothing came up on port {port}");
}

/// A particle crosses the wire, a chain of handlers answers it, and the answer
/// comes back — with `_class` intact at every hop.
///
/// The chain is the point. `net_server` opens no envelope: it hands the
/// particle to the program, and the program does authentication in one
/// handler, authorization in the next, and only then emits the *inner*
/// particle — which is held in a variable, so which handler runs is decided
/// by its class at runtime. That is the whole reason these modules carry no
/// policy of their own.
#[test]
fn a_particle_crosses_the_wire_and_a_handler_chain_answers_it() {
    let dir = workspace("round-trip");

    for mode in ["run", "build"] {
        let port = free_port();

        let server = dir.join(format!("server-{mode}.code"));
        fs::write(
            &server,
            format!(
                r#"link "net_server.so" as net

Ping {{ value }} => {{
    return Pong {{ value = value + 1 }}
}}

Authenticated {{ user, particle }} => {{
    emit particle to this get inner
    return Answered {{ user = user, inner = inner }}
}}

Impulse {{ token, app, particle }} => {{
    if token = "" {{
        return Denied {{ reason = "no token" }}
    }}
    emit Authenticated {{ user = "u-" + token, particle = particle }} to this get r
    return r
}}

emit Config {{ port = {port} }} to net get c
assert c.ok
emit Listen {{ }} to net get l
assert l.ok
"#
            ),
        )
        .expect("write the server");

        let mut child = start(&dir, mode, &server, &format!("server-{mode}"));
        await_listening(port);

        // The good path: the token survives, the app segment arrives, the
        // inner particle is dispatched by its own class, and the nested answer
        // comes back whole.
        let client = dir.join(format!("client-{mode}.code"));
        fs::write(
            &client,
            format!(
                r#"link "net_client.so" as net

emit Send {{
    url = "euglena://127.0.0.1:{port}/demo",
    particle = Impulse {{ token = "t1", particle = Ping {{ value = 41 }} }}
}} to net get answer

assert answer ∈ Answered
assert answer.user = "u-t1"
assert answer.inner ∈ Pong
assert answer.inner.value = 42
"#
            ),
        )
        .expect("write the client");
        assert!(
            run_to_end(&dir, mode, &client, &format!("client-{mode}")),
            "{mode}: the round trip did not answer as expected"
        );

        // The refused path: the same chain, stopping at the first handler.
        // Proof the program's own policy is what decides, not the module.
        let denied = dir.join(format!("denied-{mode}.code"));
        fs::write(
            &denied,
            format!(
                r#"link "net_client.so" as net

emit Send {{
    url = "euglena://127.0.0.1:{port}/demo",
    particle = Impulse {{ token = "", particle = Ping {{ value = 1 }} }}
}} to net get answer

assert answer ∈ Denied
assert answer.reason = "no token"
"#
            ),
        )
        .expect("write the denied client");
        assert!(
            run_to_end(&dir, mode, &denied, &format!("denied-{mode}")),
            "{mode}: an empty token should have been denied by the program"
        );

        // A class nothing handles is answered null rather than left to time
        // out — the sender finds out, which is the honest thing.
        let unhandled = dir.join(format!("unhandled-{mode}.code"));
        fs::write(
            &unhandled,
            format!(
                r#"link "net_client.so" as net

emit Send {{
    url = "euglena://127.0.0.1:{port}/demo",
    particle = Whatever {{ }}
}} to net get answer

assert answer = null
"#
            ),
        )
        .expect("write the unhandled client");
        assert!(
            run_to_end(&dir, mode, &unhandled, &format!("unhandled-{mode}")),
            "{mode}: an unhandled class should answer null"
        );

        let _ = child.kill();
        let _ = child.wait();
    }

    let _ = fs::remove_dir_all(&dir);
}

/// The server writes no keep-alive loop, costs nothing while idle, and ends
/// when a sender asks it to.
///
/// All three are the same mechanism: `net_server` reports itself serving while
/// its accept thread lives, the host keeps the program up for exactly that
/// long and parks rather than spinning, and `Stop` — sent from a handler, so
/// over the wire — ends the thread and lets `main` finish.
#[test]
fn the_server_idles_at_nothing_and_stop_ends_it_over_the_wire() {
    let dir = workspace("lifecycle");

    for mode in ["run", "build"] {
        let port = free_port();

        let server = dir.join(format!("server-{mode}.code"));
        fs::write(
            &server,
            format!(
                r#"link "net_server.so" as net

Ping {{ }} => {{
    return Pong {{ }}
}}

Quit {{ }} => {{
    emit Stop {{ }} to net get s
    assert s.ok
    return Bye {{ }}
}}

emit Config {{ port = {port} }} to net get c
assert c.ok
emit Listen {{ }} to net get l
assert l.ok
"#
            ),
        )
        .expect("write the server");

        let mut child = start(&dir, mode, &server, &format!("server-{mode}"));
        await_listening(port);
        let pid = child.id();

        // Idle at nothing. Asserted as a fraction of wall time with a wide
        // margin: the host parks on its own queue, so this is about 1% of a
        // core, and anything under a quarter is unambiguous on a busy machine.
        let window = Duration::from_secs(2);
        let before = cpu_seconds(pid);
        thread::sleep(window);
        let used = cpu_seconds(pid) - before;
        assert!(
            used < window.as_secs_f64() * 0.25,
            "{mode}: an idle server burned {used:.2}s of CPU over {:.0}s of doing nothing",
            window.as_secs_f64()
        );

        // Still serving after being left alone, so the host went back to
        // waiting rather than falling out of it.
        let ping = dir.join(format!("ping-{mode}.code"));
        fs::write(
            &ping,
            format!(
                r#"link "net_client.so" as net
emit Send {{ url = "euglena://127.0.0.1:{port}", particle = Ping {{ }} }} to net get r
assert r ∈ Pong
"#
            ),
        )
        .expect("write the ping client");
        assert!(
            run_to_end(&dir, mode, &ping, &format!("ping-{mode}")),
            "{mode}: still-serving check failed"
        );

        // And a sender can ask it to shut down.
        let quit = dir.join(format!("quit-{mode}.code"));
        fs::write(
            &quit,
            format!(
                r#"link "net_client.so" as net
emit Send {{ url = "euglena://127.0.0.1:{port}", particle = Quit {{ }} }} to net get r
assert r ∈ Bye
"#
            ),
        )
        .expect("write the quit client");
        assert!(
            run_to_end(&dir, mode, &quit, &format!("quit-{mode}")),
            "{mode}: Quit was not answered"
        );

        let deadline = Instant::now() + Duration::from_secs(20);
        let ended = loop {
            match child.try_wait().expect("poll the server") {
                Some(status) => break Some(status),
                None if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
                None => break None,
            }
        };
        match ended {
            Some(status) => assert!(status.success(), "{mode}: server exited with {status}"),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{mode}: the server did not end after Stop");
            }
        }
    }

    let _ = fs::remove_dir_all(&dir);
}

/// Several senders at once are all answered.
///
/// `net_server` accepts concurrently — a connection per thread, a slot per
/// request id — while dispatch into the program stays serial, because the host
/// drains on one thread and a handler may not re-enter another. So the socket
/// does not block and the answers still come back to the right senders. That
/// correlation is why the module carries `_request_id` rather than
/// `http_server`'s single pending slot.
#[test]
fn several_senders_at_once_are_each_answered() {
    let dir = workspace("concurrent");
    let port = free_port();

    let server = dir.join("server.code");
    fs::write(
        &server,
        format!(
            r#"link "net_server.so" as net

Echo {{ n }} => {{
    return Echoed {{ n = n }}
}}

emit Config {{ port = {port} }} to net get c
assert c.ok
emit Listen {{ }} to net get l
assert l.ok
"#
        ),
    )
    .expect("write the server");

    let mut child = start(&dir, "run", &server, "server");
    await_listening(port);

    // Compiled once, run many times over: each sender asserts on its own
    // number coming back, so a crossed answer fails the run.
    let senders: Vec<u32> = (1..=6).collect();
    let exes: Vec<PathBuf> = senders
        .iter()
        .map(|n| {
            let source = dir.join(format!("send-{n}.code"));
            fs::write(
                &source,
                format!(
                    r#"link "net_client.so" as net
emit Send {{ url = "euglena://127.0.0.1:{port}/a", particle = Echo {{ n = {n} }} }} to net get r
assert r ∈ Echoed
assert r.n = {n}
"#
                ),
            )
            .expect("write a sender");
            let exe = dir.join(format!("send-{n}"));
            code::compile_file(&source, code::BuildTarget::Exe, &exe, false).expect("compile");
            exe
        })
        .collect();

    let running: Vec<Child> = exes
        .iter()
        .map(|exe| {
            Command::new(exe)
                .current_dir(&dir)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn a sender")
        })
        .collect();

    for (n, mut sender) in senders.iter().zip(running) {
        let status = sender.wait().expect("wait for a sender");
        assert!(
            status.success(),
            "sender {n} did not get its own answer back"
        );
    }

    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_dir_all(&dir);
}

/// A connection that never finishes its frame must not hold the program open.
///
/// This is the other side of counting connections in `IN_FLIGHT`. Counting is
/// what lets a handler call `Stop` and still have its answer reach the caller
/// — the accept loop stops, but the program stays alive while a reply is
/// owed. Taken alone that hands any client a way to pin the process for ever:
/// connect, say nothing, and the count never comes back down.
///
/// So the socket read is bounded by the same `response_timeout_seconds` that
/// bounds waiting for the program. Here one connection stalls, a second sends
/// `Quit` — whose handler calls `Stop` — and the program has to end anyway.
/// Without the read timeout it runs until this test's patience does.
#[test]
fn a_stalled_connection_cannot_hold_the_program_open() {
    use std::io::Write;
    use std::net::TcpStream;

    let dir = workspace("stalled");
    let port = free_port();

    let server = dir.join("server.code");
    fs::write(
        &server,
        format!(
            r#"link "net_server.so" as net

Quit {{ }} => {{
    emit Stop {{ }} to net get s
    assert s.ok
    return Bye {{ }}
}}

emit Config {{ port = {port}, response_timeout_seconds = 2 }} to net get c
assert c.ok
emit Listen {{ }} to net get l
assert l.ok
"#
        ),
    )
    .expect("write the server");

    let mut child = start(&dir, "run", &server, "server-stalled");
    await_listening(port);

    // Connect and say nothing, holding it open for longer than the test.
    let _stalled = TcpStream::connect(("127.0.0.1", port)).expect("open a stalled connection");

    // A second connection asks it to quit, framed the way net_client does:
    // four big-endian length bytes, then the JSON envelope.
    let mut quit = TcpStream::connect(("127.0.0.1", port)).expect("open the quit connection");
    let body = br#"{"app":"","particle":{"_class":"Quit"}}"#;
    quit.write_all(&(body.len() as u32).to_be_bytes())
        .and_then(|()| quit.write_all(body))
        .expect("send Quit");

    // The stall is 2s of read timeout, so this is generous without being
    // indistinguishable from hanging.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match child.try_wait().expect("poll the server") {
            Some(status) => {
                assert!(status.success(), "the server exited badly: {status}");
                break;
            }
            None if Instant::now() < deadline => thread::sleep(Duration::from_millis(100)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("a stalled connection kept the program alive past Stop");
            }
        }
    }
}

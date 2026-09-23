//! The `mqtt` module against a broker — a small one written here, since a
//! test that needs Mosquitto running is a test nobody runs. It speaks just
//! enough MQTT 3.1.1 for one client: CONNECT, SUBSCRIBE (answered, and
//! followed by one retained message), PUBLISH (acknowledged, and echoed
//! back to the subscriber), PING and DISCONNECT.
//!
//! What the fixture proves: a subscription asked for before there is a
//! connection is made once there is one; `Status` says when the broker has
//! it; a retained message arrives marked so; a message the program
//! publishes comes back as the particle it named, on the program's own
//! thread; and `Disconnect` lets the program end.
//!
//! Both output modes, because a module is exactly where `code run` and
//! `code build` could differ.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

#[path = "support/modules.rs"]
mod modules;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{fs, thread};

fn build_module(name: &str) -> PathBuf {
    modules::build_so(name)
}

fn workspace(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("code-mqtt-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    fs::copy(build_module("mqtt"), dir.join("mqtt.so")).expect("copy mqtt.so");
    dir
}

// --- the broker -----------------------------------------------------------

fn read_exact(s: &mut TcpStream, n: usize) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; n];
    s.read_exact(&mut buf).ok()?;
    Some(buf)
}

/// One packet: its type-and-flags byte and its body.
fn read_packet(s: &mut TcpStream) -> Option<(u8, Vec<u8>)> {
    let first = read_exact(s, 1)?[0];
    let mut len = 0usize;
    let mut mult = 1usize;
    loop {
        let b = read_exact(s, 1)?[0];
        len += (b & 0x7f) as usize * mult;
        if b & 0x80 == 0 {
            break;
        }
        mult *= 128;
    }
    Some((first, read_exact(s, len)?))
}

fn write_packet(s: &mut TcpStream, first: u8, body: &[u8]) {
    let mut out = vec![first];
    let mut len = body.len();
    loop {
        let mut b = (len % 128) as u8;
        len /= 128;
        if len > 0 {
            b |= 0x80;
        }
        out.push(b);
        if len == 0 {
            break;
        }
    }
    out.extend_from_slice(body);
    let _ = s.write_all(&out);
}

fn mqtt_string(text: &str) -> Vec<u8> {
    let mut v = (text.len() as u16).to_be_bytes().to_vec();
    v.extend_from_slice(text.as_bytes());
    v
}

fn publish_body(topic: &str, payload: &[u8]) -> Vec<u8> {
    let mut body = mqtt_string(topic);
    body.extend_from_slice(payload);
    body
}

/// Serves one client until it disconnects or goes away. Publishes are
/// echoed straight back at QoS 0 — the client subscribed to everything
/// this test publishes, and a broker that checks filters is not the thing
/// under test.
fn serve(mut s: TcpStream) {
    s.set_read_timeout(Some(Duration::from_secs(20))).ok();
    while let Some((first, body)) = read_packet(&mut s) {
        match first >> 4 {
            1 => write_packet(&mut s, 0x20, &[0x00, 0x00]),
            8 => {
                let pkid = [body[0], body[1]];
                write_packet(&mut s, 0x90, &[pkid[0], pkid[1], 0x01]);
                // What the broker kept for this filter: sent retained.
                write_packet(
                    &mut s,
                    0x31,
                    &publish_body("code/test/kept", b"from before"),
                );
            }
            10 => write_packet(&mut s, 0xB0, &[body[0], body[1]]),
            3 => {
                let qos = (first >> 1) & 0x03;
                let topic_len = u16::from_be_bytes([body[0], body[1]]) as usize;
                let topic = String::from_utf8_lossy(&body[2..2 + topic_len]).into_owned();
                let mut at = 2 + topic_len;
                if qos > 0 {
                    write_packet(&mut s, 0x40, &[body[at], body[at + 1]]);
                    at += 2;
                }
                let payload = body[at..].to_vec();
                write_packet(&mut s, 0x30, &publish_body(&topic, &payload));
            }
            12 => write_packet(&mut s, 0xD0, &[]),
            14 => return,
            _ => {}
        }
    }
}

fn broker() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("port").port();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            thread::spawn(move || serve(stream));
        }
    });
    port
}

// --- the program ------------------------------------------------------------

fn fixture(port: u16) -> String {
    r#"link "mqtt.so" as bus

heard = []

Spoke { topic, payload, retained, from } =>
    heard += [{ topic = topic, payload = payload, retained = retained, from = from }]
    return Ack {}

emit Config { url = "mqtt://127.0.0.1:PORT", username = "home", password = "secret" } to bus get c
assert c.ok
| A whole particle as `then`: its fields ride along with every message.
emit Subscribe { topic = "code/test/#", then = { _class = "Spoke", from = "the house" } } to bus get s
assert s.ok

| Not connected yet: refused now, not queued.
emit Publish { topic = "code/test/one", payload = "early" } to bus get early
assert early ∈ Exception

| The broker registers the filter; only then is a publish worth making.
spins = 0
ready = false
loop
    spins += 1
    emit Status {} to bus get st
    if st.subscribed = ["code/test/#"]
        ready = st.connected
        break
    if spins > 4000000, break
assert ready

emit Publish { topic = "code/test/one", payload = "hello" } to bus get p
assert p.ok

loop
    spins += 1
    emit Length { value = heard } to core get n
    if n.value = 2, break
    if spins > 8000000, break

| The retained message first, marked so; then our own, echoed.
assert heard[0].topic = "code/test/kept"
assert heard[0].payload = "from before"
assert heard[0].retained = true
assert heard[0].from = "the house"
assert heard[1].topic = "code/test/one"
assert heard[1].payload = "hello"
assert heard[1].retained = false

emit Disconnect {} to bus get d
assert d.ok
"#
    .replace("PORT", &port.to_string())
}

fn run_to_end(dir: &Path, mode: &str, source: &Path) -> bool {
    let mut child = if mode == "run" {
        Command::new(env!("CARGO_BIN_EXE_code"))
            .arg("run")
            .arg(source)
            .current_dir(dir)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn code run")
    } else {
        let exe = dir.join("program");
        code::compile_file(source, code::BuildTarget::Exe, &exe, false).expect("compile");
        Command::new(&exe)
            .current_dir(dir)
            .env("CODE_CHECK_LEAKS", "1")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn the compiled program")
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match child.try_wait().expect("poll") {
            Some(status) => return status.success(),
            None if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{mode}: the program never finished — Disconnect did not let it end");
            }
        }
    }
}

#[test]
fn messages_come_back_as_the_particle_the_program_named() {
    let port = broker();
    let dir = workspace("round-trip");
    let source = dir.join("program.code");
    fs::write(&source, fixture(port)).expect("write fixture");
    for mode in ["run", "build"] {
        assert!(
            run_to_end(&dir, mode, &source),
            "{mode}: the fixture failed"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

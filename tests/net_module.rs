//! The `net` module against a real HTTP server.
//!
//! The `net_*.code` fixtures cover what can be asserted with nothing
//! listening — a refused connection, and the misuse that aborts. Everything
//! that needs an actual response lives here instead, because a fixture
//! cannot be told which port to talk to.
//!
//! The server is a few lines of `std::net` rather than a dependency, and it
//! binds port 0 so two runs of this test never collide. Nothing here leaves
//! loopback: the suite still passes on a machine with no network at all.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Exactly the bytes `net_module.code` asserts on — 26 of them, which is
/// what makes the `max_body_bytes` boundary checks below meaningful.
const HELLO: &str = "hello from the test server";

fn respond(stream: &mut TcpStream, status: u16, reason: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// One request, answered by path. Deliberately not a real HTTP parser: it
/// reads the request line, the headers it cares about, and the body if one
/// was announced.
fn handle(mut stream: TcpStream) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();

    let mut content_length = 0usize;
    let mut probe_header = String::new();
    let mut content_type = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line == "\r\n" || line.is_empty() {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = lower.strip_prefix("content-type:") {
            content_type = v.trim().to_string();
        } else if let Some(v) = lower.strip_prefix("x-probe:") {
            probe_header = v.trim().to_string();
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 && reader.read_exact(&mut body).is_err() {
        return;
    }
    let body = String::from_utf8_lossy(&body).to_string();

    match path.as_str() {
        "/hello" => respond(&mut stream, 200, "OK", HELLO),
        "/echo" => respond(
            &mut stream,
            201,
            "Created",
            &format!("echo:{body} ct={content_type}"),
        ),
        "/probe" => respond(&mut stream, 200, "OK", &probe_header),
        // A 500 is a perfectly good *response*: `ok` says whether one
        // arrived, not whether the server liked the request.
        "/boom" => respond(&mut stream, 500, "Internal Server Error", "boom"),
        _ => respond(&mut stream, 404, "Not Found", "no"),
    }
}

/// Build the module and return the `.so`'s path. Shares
/// `crates/modules/net`'s target directory with the fixture harness, which
/// builds the same crate — cargo's own lock serialises the two.
fn build_net_module() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/net");
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        .status()
        .expect("run cargo for crates/modules/net");
    assert!(status.success(), "cargo failed to build crates/modules/net");
    crate_dir.join("target/release/libnet.so")
}

#[test]
fn get_and_post_against_a_real_server() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("read bound port").port();
    // Detached: the test process exiting is what stops it, so there is no
    // shutdown to coordinate and no count of requests to keep in step with
    // the program below.
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || handle(stream));
        }
    });

    let dir = std::env::temp_dir().join(format!("code-net-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    // Copied in beside the program: `link` resolves against the linking
    // file's own directory first, so this needs no install or search path.
    fs::copy(build_net_module(), dir.join("net.so")).expect("copy net.so");

    let program = format!(
        r#"link "net.so" as net

emit Get {{ "url": "http://127.0.0.1:{port}/hello" }} to net get r
assert r.ok
assert r.status = 200
assert r.body = "{HELLO}"

emit Post {{ "url": "http://127.0.0.1:{port}/echo", "body": "ping", "content_type": "text/plain" }} to net get p
assert p.ok
assert p.status = 201
assert p.body = "echo:ping ct=text/plain"

-- Headers reach the server as headers.
emit Get {{ "url": "http://127.0.0.1:{port}/probe", "headers": {{ "X-Probe": "seen" }} }} to net get h
assert h.ok
assert h.body = "seen"

-- A 500 arrived, so `ok` is true; `status` is what went wrong.
emit Get {{ "url": "http://127.0.0.1:{port}/boom" }} to net get b
assert b.ok
assert b.status = 500
assert b.body = "boom"

-- The cap is exact: {} bytes is fine, one fewer is not, and going over
-- fails the request rather than handing back a truncated body that looks
-- whole.
emit Get {{ "url": "http://127.0.0.1:{port}/hello", "max_body_bytes": {} }} to net get exact
assert exact.ok
assert exact.body = "{HELLO}"

emit Get {{ "url": "http://127.0.0.1:{port}/hello", "max_body_bytes": {} }} to net get too_big
assert not too_big.ok
assert too_big.status = 0
assert too_big.body = "response body exceeds max_body_bytes ({})"
"#,
        HELLO.len(),
        HELLO.len(),
        HELLO.len() - 1,
        HELLO.len() - 1,
    );
    let source = dir.join("net_module.code");
    fs::write(&source, &program).expect("write program");

    // Both output modes, because the run/build invariant is the rule this
    // repo cares most about and a module is exactly where the two paths
    // differ most.
    code::run_file(&source).expect("interpret");

    let exe = dir.join("net_module");
    code::compile_file(&source, code::BuildTarget::Exe, &exe, false).expect("compile");
    let status = Command::new(&exe)
        .current_dir(&dir)
        .status()
        .expect("run compiled program");
    assert!(
        status.success(),
        "compiled program failed its own assertions"
    );

    let _ = fs::remove_dir_all(&dir);
}

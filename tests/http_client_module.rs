//! The `http_client` module against a real HTTP server.
//!
//! The `http_client_*.code` fixtures cover what can be asserted with nothing
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

/// Exactly the bytes `http_client_module.code` asserts on — 26 of them, which is
/// what makes the `max_body_bytes` boundary checks below meaningful.
const HELLO: &str = "hello from the test server";

/// `head` suppresses the body while still announcing its length, which is
/// what HEAD means and what ureq expects to parse — a body sent anyway would
/// be read as the start of the next response.
fn respond_to(stream: &mut TcpStream, status: u16, reason: &str, body: &str, head: bool) {
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        if head { "" } else { body }
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
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    // Bound to the request, so every `respond` below observes HEAD without
    // being told about it.
    let respond = |stream: &mut TcpStream, status, reason: &str, body: &str| {
        respond_to(stream, status, reason, body, method == "HEAD");
    };

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
        // Echoes the verb back, which is the only way to prove from the
        // program's side that `Put` sent PUT and not something else.
        "/method" => respond(&mut stream, 200, "OK", &format!("{method}:{body}")),
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
/// `crates/modules/http_client`'s target directory with the fixture harness, which
/// builds the same crate — cargo's own lock serialises the two.
fn build_http_client_module() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/http_client");
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        .status()
        .expect("run cargo for crates/modules/http_client");
    assert!(
        status.success(),
        "cargo failed to build crates/modules/http_client"
    );
    crate_dir.join("target/release/libhttp_client.so")
}

#[test]
fn every_method_against_a_real_server() {
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

    let dir = std::env::temp_dir().join(format!("code-http-client-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    // Copied in beside the program: `link` resolves against the linking
    // file's own directory first, so this needs no install or search path.
    fs::copy(build_http_client_module(), dir.join("http_client.so")).expect("copy http_client.so");

    let program = format!(
        r#"link "http_client.so" as http

| Every completed request pushes a `Log`; a request that never completed
| pushes an `Exception` instead. Both are dispatched to these handlers
| between top-level statements. Counted rather than inspected, so the
| assertion does not depend on the exact wording of a message.
let logs = 0
let exceptions = 0

Log {{ source, level, message }} => {{
    logs = logs + 1
}}

Exception {{ source, message }} => {{
    exceptions = exceptions + 1
}}

emit Get {{ url = "http://127.0.0.1:{port}/hello" }} to http get r
assert r.ok
assert r.status = 200
assert r.body = "{HELLO}"

emit Post {{ url = "http://127.0.0.1:{port}/echo", body = "ping", content_type = "text/plain" }} to http get p
assert p.ok
assert p.status = 201
assert p.body = "echo:ping ct=text/plain"

| Headers reach the server as headers.
emit Get {{ url = "http://127.0.0.1:{port}/probe", headers = {{ "X-Probe" = "seen" }} }} to http get h
assert h.ok
assert h.body = "seen"

| A 500 arrived, so `ok` is true; `status` is what went wrong.
emit Get {{ url = "http://127.0.0.1:{port}/boom" }} to http get b
assert b.ok
assert b.status = 500
assert b.body = "boom"

| One particle per HTTP method, and the server echoes the verb it actually
| received — so this proves the routing, not just that a request happened.
| The three body-carrying methods send one; the four others do not.
emit Put {{ url = "http://127.0.0.1:{port}/method", body = "p" }} to http get put
assert put.ok
assert put.body = "PUT:p"

emit Patch {{ url = "http://127.0.0.1:{port}/method", body = "q" }} to http get patch
assert patch.ok
assert patch.body = "PATCH:q"

emit Delete {{ url = "http://127.0.0.1:{port}/method" }} to http get del
assert del.ok
assert del.body = "DELETE:"

emit Options {{ url = "http://127.0.0.1:{port}/method" }} to http get opts
assert opts.ok
assert opts.body = "OPTIONS:"

| HEAD gets the status and the headers, never a body. Empty is the right
| answer, not a lost one.
emit Head {{ url = "http://127.0.0.1:{port}/hello" }} to http get head
assert head.ok
assert head.status = 200
assert head.body = ""

| The cap is exact: {} bytes is fine, one fewer is not, and going over
| fails the request rather than handing back a truncated body that looks
| whole.
emit Get {{ url = "http://127.0.0.1:{port}/hello", max_body_bytes = {} }} to http get exact
assert exact.ok
assert exact.body = "{HELLO}"

emit Get {{ url = "http://127.0.0.1:{port}/hello", max_body_bytes = {} }} to http get too_big
assert not too_big.ok
assert too_big.status = 0
assert too_big.body = "response body exceeds max_body_bytes ({})"

| Exact, because the split is the interesting part: ten requests got a
| response and logged it — including the 500, which is news rather than a
| fault — and only the one that went over the cap raised an Exception,
| since that is the only request here that never produced a usable body.
assert logs = 10
assert exceptions = 1
"#,
        HELLO.len(),
        HELLO.len(),
        HELLO.len() - 1,
        HELLO.len() - 1,
    );
    let source = dir.join("http_client_module.code");
    fs::write(&source, &program).expect("write program");

    // Both output modes, because the run/build invariant is the rule this
    // repo cares most about and a module is exactly where the two paths
    // differ most.
    code::run_file(&source).expect("interpret");

    let exe = dir.join("http_client_module");
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

//! The `http_server` module, answered by a real client.
//!
//! A `.code` fixture cannot test this. The program has to still be running
//! when the request arrives, and it cannot make the request itself: a
//! self-request would block inside `http_client`'s dispatch, and the drain
//! that would deliver the `Request` to a handler only runs between the
//! program's own statements — so the program would be waiting for a handler
//! that cannot start until the request it is blocked on finishes. That is a
//! property of a single-threaded program, not a bug in the module. Hence a
//! client from outside the process, which is what this file is.
//!
//! Both output modes, because a module is exactly where `code run` and
//! `code build` differ most.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use std::{fs, thread};

/// Build the module and return the `.so`'s path.
fn build_module() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/http_server");
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        .status()
        .expect("run cargo for crates/modules/http_server");
    assert!(status.success(), "cargo failed to build http_server");
    crate_dir.join("target/release/libhttp_server.so")
}

/// A port nothing is listening on: bind zero, read what the OS chose, let go.
/// The window between letting go and the program binding it is a race in
/// principle; in practice the program `assert`s that its `Listen` succeeded,
/// so losing that race is a loud failure rather than a confusing one.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind loopback")
        .local_addr()
        .expect("read bound port")
        .port()
}

/// One request, one response, spoken by hand — a dependency-free client for a
/// server that promises `Connection: close` and nothing else.
fn request(port: u16, method: &str, path: &str, body: &str) -> String {
    request_with(port, method, path, &[], body)
}

/// `request`, plus extra header lines (`("Authorization", "Bearer x")`).
fn request_with(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    let extra: String = headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect();
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(mut stream) => {
                let request = format!(
                    "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(request.as_bytes()).expect("write request");
                stream.flush().expect("flush");
                let mut response = String::new();
                stream.read_to_string(&mut response).expect("read response");
                return response;
            }
            // The program is still starting: `Listen` happens a few
            // statements in, and the compiled binary has to be exec'd first.
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            Err(e) => panic!("could not reach the program's server on {port}: {e}"),
        }
    }
}

fn status_of(response: &str) -> u16 {
    response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0)
}

fn body_of(response: &str) -> String {
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default()
}

/// Runs `program` under both output modes, handing each a fresh port, and
/// calls `check` with the port while it is up. The program is killed after —
/// it is a daemon, and the only way it ends.
fn serving(tag: &str, program: &str, check: impl Fn(u16)) {
    let dir = std::env::temp_dir().join(format!("code-http-server-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    // Beside the program: `link` resolves against the linking file's own
    // directory first, so this needs no install and no search path.
    fs::copy(build_module(), dir.join("http_server.so")).expect("copy http_server.so");

    for mode in ["run", "build"] {
        let port = free_port();
        let source = dir.join(format!("{mode}.code"));
        fs::write(&source, program.replace("PORT", &port.to_string())).expect("write program");

        let mut child: Child = if mode == "run" {
            Command::new(env!("CARGO_BIN_EXE_code"))
                .arg("run")
                .arg(&source)
                .current_dir(&dir)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn code run")
        } else {
            let exe = dir.join(mode);
            code::compile_file(&source, code::BuildTarget::Exe, &exe, false).expect("compile");
            Command::new(&exe)
                .current_dir(&dir)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn the compiled program")
        };

        check(port);

        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_handlers_return_value_is_the_http_response() {
    // No `Respond`, no request id: the handler answers by returning, exactly
    // as it would to any other particle. The id the module needs to match an
    // answer to a request never reaches the program at all.
    let program = r#"link "http_server.so" as srv

Request { method, path, query, body } => {
    if path = "/echo" {
        return Response { status = 201, body = body }
    }
    if path = "/query" {
        return Response { status = 200, body = query }
    }
    return Response { status = 200, body = "$method $path" }
}

emit Config { port = PORT } to srv get c
assert c.ok
emit Listen { } to srv get l
assert l.ok
assert l.port = PORT

loop {
}
"#;
    serving("answers", program, |port| {
        let r = request(port, "GET", "/hello", "");
        assert_eq!(status_of(&r), 200);
        assert_eq!(body_of(&r), "GET /hello");

        // A body arrives, and the status the handler chose is the status sent.
        let r = request(port, "POST", "/echo", "ping");
        assert_eq!(status_of(&r), 201);
        assert_eq!(body_of(&r), "ping");

        // The query is handed over raw, split from the path.
        let r = request(port, "GET", "/query?a=1&b=2", "");
        assert_eq!(body_of(&r), "a=1&b=2");

        // Still serving after all of that: one request at a time, but not one
        // request only.
        let r = request(port, "GET", "/again", "");
        assert_eq!(body_of(&r), "GET /again");
    });
}

#[test]
fn request_headers_reach_the_handler_by_lowercased_name() {
    // A gateway pattern: read a bearer token off the request and answer
    // differently for it. Header names are lowercased; a repeat is joined.
    let program = r#"link "http_server.so" as srv

Request { method, path, headers } => {
    if path = "/whoami" {
        if headers.authorization = "Bearer let-me-in" {
            return Response { status = 200, body = "welcome" }
        }
        return Response { status = 401, body = "no" }
    }
    if path = "/type" {
        if headers["content-type"] = "application/json" {
            return Response { status = 200, body = "json" }
        }
        return Response { status = 200, body = "other" }
    }
    if path = "/accept" {
        if headers.accept = "text/html, application/json" {
            return Response { status = 200, body = "joined" }
        }
        return Response { status = 200, body = "not joined" }
    }
    return Response { status = 200, body = "ok" }
}

emit Config { port = PORT } to srv get c
emit Listen { } to srv get l
assert l.ok

loop {
}
"#;
    serving("headers", program, |port| {
        let r = request_with(
            port,
            "GET",
            "/whoami",
            &[("Authorization", "Bearer let-me-in")],
            "",
        );
        assert_eq!(status_of(&r), 200);
        assert_eq!(body_of(&r), "welcome");

        // Same header, name sent in a different case — still found.
        let r = request_with(
            port,
            "GET",
            "/whoami",
            &[("AUTHORIZATION", "Bearer nope")],
            "",
        );
        assert_eq!(status_of(&r), 401);

        let r = request_with(port, "GET", "/whoami", &[], "");
        assert_eq!(status_of(&r), 401);

        let r = request_with(
            port,
            "POST",
            "/type",
            &[("Content-Type", "application/json")],
            "{}",
        );
        assert_eq!(body_of(&r), "json");

        // A header sent twice is joined with ", ".
        let r = request_with(
            port,
            "GET",
            "/accept",
            &[("Accept", "text/html"), ("Accept", "application/json")],
            "",
        );
        assert_eq!(body_of(&r), "joined");
    });
}

#[test]
fn no_handler_is_a_404_rather_than_a_hang() {
    // A program that links the server and never handles `Request`. The push
    // finds no handler, the host answers null, and the module turns that into
    // the status that means "nobody claimed this".
    let program = r#"link "http_server.so" as srv

emit Config { port = PORT } to srv get c
emit Listen { } to srv get l
assert l.ok

loop {
}
"#;
    serving("unhandled", program, |port| {
        let r = request(port, "GET", "/anything", "");
        assert_eq!(status_of(&r), 404);
    });
}

#[test]
fn config_is_frozen_once_listening() {
    // A `Config` sent *after* `Listen` is an `Exception`, not a silent no-op
    // — the socket is already bound, so the new config would be a lie. The
    // program `assert`s that itself, so a regression fails the run before any
    // request is made.
    let program = r#"link "http_server.so" as srv

Request { method, path } => {
    return Response { status = 200, body = "up" }
}

emit Config { port = PORT } to srv get c
assert c.ok
emit Listen { } to srv get l
assert l.ok

emit Config { port = 1 } to srv get late
assert late ∈ Exception

loop {
}
"#;
    serving("config-frozen", program, |port| {
        assert_eq!(status_of(&request(port, "GET", "/", "")), 200);
    });
}

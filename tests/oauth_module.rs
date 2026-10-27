//! The `oauth` module, against a fake provider.
//!
//! `AuthUrl` is pure and covered by `tests/oauth_auth_url.code`. The
//! `ExchangeCode` round trip — POST the token endpoint, then GET userinfo
//! with the bearer token — needs an HTTP server on the other end. This
//! stands a minimal one up on loopback and drives the module through both
//! output modes.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

#[path = "support/modules.rs"]
mod modules;

use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use std::{fs, thread};

fn build_module() -> PathBuf {
    modules::build_so("oauth")
}

/// The two answers a provider with a userinfo endpoint gives.
const GOOGLE_LIKE: [(&str, &str); 2] = [
    (
        "POST /token",
        r#"{"access_token":"at-123","refresh_token":"rt-456","token_type":"Bearer"}"#,
    ),
    (
        "GET /userinfo",
        r#"{"sub":"user-9","email":"u@example.com","name":"Ada L","picture":"https://img/u.png"}"#,
    ),
];

/// Entra ID's shape: no userinfo asked for, and the roles only in the
/// id_token. Its payload is
/// `{"sub":"e-1","name":"Karla","preferred_username":"karla@hfc.example","roles":["hfc_admin"],"tid":"t-7"}`,
/// with no `email`, as Entra sends for some accounts.
const ENTRA_LIKE: [(&str, &str); 1] = [(
    "POST /token",
    r#"{"access_token":"at-e","token_type":"Bearer","id_token":"eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJlLTEiLCJuYW1lIjoiS2FybGEiLCJwcmVmZXJyZWRfdXNlcm5hbWUiOiJrYXJsYUBoZmMuZXhhbXBsZSIsInJvbGVzIjpbImhmY19hZG1pbiJdLCJ0aWQiOiJ0LTcifQ.c2lnbmF0dXJl"}"#,
)];

/// Serve exactly the requests given — `POST /token`, then `GET /userinfo`
/// when there is one — with canned JSON, and hang up. Enough of HTTP/1.1 for `ureq` to be happy.
///
/// Each request is read whole before the answer goes out, and the answer is
/// ended with a half-close rather than by dropping the socket. Hanging up
/// with part of a request still unread makes the kernel reset the
/// connection, and a reset throws away the answer the client had not read
/// yet: the module then saw "Peer disconnected" in about one run in seven
/// (2026-10-24). A connection that never comes no longer blocks forever
/// either; after a deadline the server gives up, and the test fails on the
/// program's own answer instead of hanging.
fn fake_provider(listener: TcpListener, answers: &[(&str, &str)]) {
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    for &(path_check, json) in answers {
        let Some(mut stream) = accept_within(&listener, Duration::from_secs(20)) else {
            return;
        };
        let head = read_request(&mut stream);
        assert!(
            head.starts_with(path_check),
            "expected `{path_check}`, got:\n{head}"
        );
        respond(&mut stream, json);
        let _ = stream.shutdown(Shutdown::Write);
        // Until the client closes its end: nothing it sends is left unread.
        let mut rest = [0u8; 256];
        while matches!(stream.read(&mut rest), Ok(n) if n > 0) {}
    }
}

fn accept_within(listener: &TcpListener, wait: Duration) -> Option<TcpStream> {
    let deadline = Instant::now() + wait;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).expect("blocking stream");
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .expect("read timeout");
                return Some(stream);
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return None,
        }
    }
}

/// The request line and headers, and as much body as `Content-Length` says.
fn read_request(stream: &mut TcpStream) -> String {
    let mut got = Vec::new();
    let mut buf = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = got.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        match stream.read(&mut buf) {
            Ok(n) if n > 0 => got.extend_from_slice(&buf[..n]),
            _ => return String::from_utf8_lossy(&got).into_owned(),
        }
    };
    let head = String::from_utf8_lossy(&got[..head_end]).into_owned();
    let length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while got.len() < head_end + length {
        match stream.read(&mut buf) {
            Ok(n) if n > 0 => got.extend_from_slice(&buf[..n]),
            _ => break,
        }
    }
    head
}

fn respond(stream: &mut TcpStream, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

fn with_provider(
    tag: &str,
    answers: &'static [(&'static str, &'static str)],
    program: &str,
    check: impl Fn(bool),
) {
    let dir = std::env::temp_dir().join(format!("code-oauth-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    fs::copy(build_module(), dir.join("oauth.so")).expect("copy oauth.so");

    for mode in ["run", "build"] {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || fake_provider(listener, answers));

        let source = dir.join(format!("{mode}.code"));
        fs::write(&source, program.replace("PORT", &port.to_string())).expect("write program");

        let ok = if mode == "run" {
            Command::new(env!("CARGO_BIN_EXE_code"))
                .args(["run", source.to_str().unwrap()])
                .current_dir(&dir)
                .status()
                .expect("spawn code run")
                .success()
        } else {
            let exe = dir.join(mode);
            code::compile_file(&source, code::BuildTarget::Exe, &exe, false).expect("compile");
            Command::new(&exe)
                .current_dir(&dir)
                .status()
                .expect("spawn compiled program")
                .success()
        };
        let _ = server.join();
        check(ok);
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn exchange_code_returns_the_identity_and_tokens() {
    // The program asserts the shape itself, so `check` only has to confirm
    // it exited zero (both the HTTP calls landed and the claims matched).
    let program = r#"link "oauth.so" as oauth

emit Config {
    client_id = "cid"
    client_secret = "sec"
    redirect_uri = "https://app/cb"
    auth_url = "http://127.0.0.1:PORT/auth"
    token_url = "http://127.0.0.1:PORT/token"
    userinfo_url = "http://127.0.0.1:PORT/userinfo"
} to oauth get c
assert c.ok

emit ExchangeCode { code = "auth-code-here" } to oauth get id
assert id ∈ Identity
assert id.sub = "user-9"
assert id.email = "u@example.com"
assert id.name = "Ada L"
assert id.picture = "https://img/u.png"
assert id.access_token = "at-123"
assert id.refresh_token = "rt-456"
"#;
    with_provider("exchange", &GOOGLE_LIKE, program, |ok| {
        assert!(ok, "the program should exit zero — ExchangeCode round trip");
    });
}

/// Entra ID: the id_token comes back raw and decoded, its roles readable,
/// and with no userinfo endpoint the identity is read from its claims —
/// `email` empty where Entra left it out, the address in
/// `preferred_username` for the caller to take.
#[test]
fn exchange_code_returns_the_id_token_and_its_claims() {
    let program = r#"link "oauth.so" as oauth

emit Config {
    client_id = "cid"
    client_secret = "sec"
    redirect_uri = "https://app/cb"
    auth_url = "http://127.0.0.1:PORT/auth"
    token_url = "http://127.0.0.1:PORT/token"
    scope = "openid email profile"
} to oauth get c
assert c.ok

emit ExchangeCode { code = "auth-code-here" } to oauth get id
assert id ∈ Identity
assert id.access_token = "at-e"
assert id.refresh_token = ""
assert id.sub = "e-1"
assert id.name = "Karla"
assert id.email = ""
assert id.claims.preferred_username = "karla@hfc.example"
assert id.claims.roles = ["hfc_admin"]
assert id.claims.tid = "t-7"
assert id.id_token[0, 20) = "eyJhbGciOiJSUzI1NiIs"
"#;
    with_provider("entra", &ENTRA_LIKE, program, |ok| {
        assert!(ok, "the program should exit zero — id_token claims read");
    });
}

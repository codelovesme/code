//! The `oauth` module, against a fake provider.
//!
//! `AuthUrl` is pure and covered by `tests/oauth_auth_url.code`. The
//! `ExchangeCode` round trip — POST the token endpoint, then GET userinfo
//! with the bearer token — needs an HTTP server on the other end. This
//! stands a minimal one up on loopback and drives the module through both
//! output modes.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{fs, thread};

fn build_module() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/oauth");
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        .status()
        .expect("run cargo for crates/modules/oauth");
    assert!(status.success(), "cargo failed to build oauth");
    crate_dir.join("target/release/liboauth.so")
}

/// Serve exactly two requests — `POST /token`, then `GET /userinfo` — with
/// canned JSON, and hang up. Enough of HTTP/1.1 for `ureq` to be happy.
fn fake_provider(listener: TcpListener) {
    for (path_check, json) in [
        (
            "POST /token",
            r#"{"access_token":"at-123","refresh_token":"rt-456","token_type":"Bearer"}"#,
        ),
        (
            "GET /userinfo",
            r#"{"sub":"user-9","email":"u@example.com","name":"Ada L","picture":"https://img/u.png"}"#,
        ),
    ] {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).unwrap_or(0);
        let head = String::from_utf8_lossy(&buf[..n]);
        assert!(
            head.starts_with(path_check),
            "expected `{path_check}`, got:\n{head}"
        );
        // Drain a POST body if the request line promised one — small and
        // already in `buf`, so nothing more to read for this test.
        let _ = &head;
        respond(&mut stream, json);
    }
}

fn respond(stream: &mut TcpStream, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

fn with_provider(tag: &str, program: &str, check: impl Fn(bool)) {
    let dir = std::env::temp_dir().join(format!("code-oauth-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    fs::copy(build_module(), dir.join("oauth.so")).expect("copy oauth.so");

    for mode in ["run", "build"] {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || fake_provider(listener));

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
    client_id = "cid",
    client_secret = "sec",
    redirect_uri = "https://app/cb",
    auth_url = "http://127.0.0.1:PORT/auth",
    token_url = "http://127.0.0.1:PORT/token",
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
    with_provider("exchange", program, |ok| {
        assert!(ok, "the program should exit zero — ExchangeCode round trip");
    });
}

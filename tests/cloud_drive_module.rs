//! The `cloud_drive` module, against a fake Google Drive.
//!
//! `AuthUrl` and the guard paths are pure and covered by
//! `tests/cloud_drive_error_paths.code`. The OAuth exchange and the five
//! file operations each need an HTTP server on the other end; this stands a
//! minimal Drive up on loopback — just enough of the `oauth2` and
//! `drive/v3` surface for one round trip — and drives the module through
//! both output modes.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{fs, thread};

fn build_module() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/cloud_drive");
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        .status()
        .expect("run cargo for crates/modules/cloud_drive");
    assert!(status.success(), "cargo failed to build cloud_drive");
    crate_dir.join("target/release/libcloud_drive.so")
}

const FILE_BODY: &str = "hello world";

/// One request: the method, the path (with query), and the body.
fn read_request(stream: &mut TcpStream) -> (String, String, Vec<u8>) {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    // Read the head byte at a time until the blank line — small requests,
    // and it keeps the body boundary exact.
    while !buf.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(0) | Err(_) => break,
            Ok(_) => buf.push(byte[0]),
        }
    }
    let head = String::from_utf8_lossy(&buf).into_owned();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();

    let content_length = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            (k.trim().eq_ignore_ascii_case("content-length"))
                .then(|| v.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        stream.read_exact(&mut body).expect("read request body");
    }
    (method, path, body)
}

fn send(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn json(stream: &mut TcpStream, body: &str) {
    send(stream, "200 OK", "application/json", body.as_bytes());
}

/// A fake Drive that answers the handful of endpoints the module calls.
/// Loops forever; the thread dies with the test process.
fn fake_drive(listener: TcpListener) {
    for incoming in listener.incoming() {
        let Ok(mut stream) = incoming else { continue };
        let (method, path, _body) = read_request(&mut stream);

        match (method.as_str(), path.as_str()) {
            ("POST", "/token") => json(
                &mut stream,
                r#"{"access_token":"at-1","refresh_token":"rt-1","expires_in":3600,"token_type":"Bearer"}"#,
            ),
            ("GET", "/oauth2/v3/userinfo") => {
                json(&mut stream, r#"{"email":"user@example.com","sub":"u-1"}"#)
            }
            ("GET", p) if p.starts_with("/drive/v3/about") => json(
                &mut stream,
                r#"{"storageQuota":{"limit":"1000","usage":"400"},"user":{"emailAddress":"user@example.com"}}"#,
            ),
            ("POST", p) if p.starts_with("/upload/drive/v3/files") => json(
                &mut stream,
                r#"{"id":"f1","name":"hello.txt","mimeType":"text/plain","size":"11","webViewLink":"https://drive.example/f1"}"#,
            ),
            ("GET", p) if p.starts_with("/drive/v3/files?") => json(
                &mut stream,
                r#"{"files":[{"id":"f1","name":"hello.txt","mimeType":"text/plain","size":"11","webViewLink":"https://drive.example/f1"}]}"#,
            ),
            ("GET", p) if p.starts_with("/drive/v3/files/f1") && p.contains("alt=media") => {
                send(&mut stream, "200 OK", "text/plain", FILE_BODY.as_bytes())
            }
            ("GET", p) if p.starts_with("/drive/v3/files/f1") => json(
                &mut stream,
                r#"{"id":"f1","name":"hello.txt","mimeType":"text/plain"}"#,
            ),
            ("DELETE", "/drive/v3/files/f1") => {
                send(&mut stream, "204 No Content", "text/plain", b"")
            }
            ("DELETE", "/drive/v3/files/missing") => send(
                &mut stream,
                "404 Not Found",
                "application/json",
                br#"{"error":{"message":"not found"}}"#,
            ),
            _ => send(
                &mut stream,
                "500 Internal Server Error",
                "text/plain",
                path.as_bytes(),
            ),
        }
    }
}

#[test]
fn the_oauth_exchange_and_file_operations_round_trip() {
    let dir = std::env::temp_dir().join(format!("code-clouddrive-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    fs::copy(build_module(), dir.join("cloud_drive.so")).expect("copy cloud_drive.so");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || fake_drive(listener));

    let program = format!(
        r#"link "cloud_drive.so" as drive

emit Config {{
    client_id = "cid", client_secret = "secret",
    redirect_uri = "https://app.example/cb",
    auth_url = "http://127.0.0.1:{port}/auth",
    token_url = "http://127.0.0.1:{port}/token",
    api_base = "http://127.0.0.1:{port}"
}} to drive get c
assert c.ok

emit ExchangeCode {{ code = "auth-code" }} to drive get t
assert t ∈ Tokens
assert t.account_email = "user@example.com"
assert t.access_token = "at-1"
assert t.refresh_token = "rt-1"
assert t.expires_in = 3600

emit GetQuota {{ access_token = t.access_token }} to drive get q
assert q.total = 1000
assert q.used = 400
assert q.available = 600
assert q.account_email = "user@example.com"

emit UploadFile {{
    access_token = t.access_token, file_name = "hello.txt",
    data = "hello world", content_type = "text/plain"
}} to drive get up
assert up ∈ RemoteFile
assert up.file_id = "f1"
assert up.file_name = "hello.txt"
assert up.size = 11
assert up.web_view_url = "https://drive.example/f1"

emit ListFiles {{ access_token = t.access_token }} to drive get l
assert l.count = 1
assert l.files[0].file_id = "f1"
assert l.files[0].file_name = "hello.txt"

emit DownloadFile {{ access_token = t.access_token, file_id = "f1" }} to drive get d
assert d ∈ FileContent
assert d.file_name = "hello.txt"
assert d.content_type = "text/plain"
assert d.data = "hello world"

emit DownloadFile {{ access_token = t.access_token, file_id = "f1", base64 = true }} to drive get d64
assert d64.data = "aGVsbG8gd29ybGQ="

emit DeleteFile {{ access_token = t.access_token, file_id = "f1" }} to drive get del
assert del.existed

emit DeleteFile {{ access_token = t.access_token, file_id = "missing" }} to drive get del2
assert del2.existed = false
"#
    );

    for mode in ["run", "build"] {
        let source = dir.join(format!("{mode}.code"));
        fs::write(&source, &program).expect("write program");
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
        assert!(
            ok,
            "{mode} mode: the cloud_drive round trip exited non-zero"
        );
    }

    let _ = fs::remove_dir_all(&dir);
}

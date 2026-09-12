//! The `azure_mailer` module, against a stand-in for Azure.
//!
//! A `.code` fixture can only reach the error paths
//! (`tests/azure_mailer_error_paths.code`) — a successful `Send` needs
//! something to accept the message. This stands a minimal HTTP server up on
//! loopback, runs a program that `Config`s and `Send`s to it through **both**
//! output modes, and then checks the two things that are easy to get wrong
//! and impossible to see from the outside: the body Azure would have
//! received, and the **signature**, recomputed here from the same key.
//!
//! Recomputing the signature is the point. A wrong one is not a crash and
//! not a wrong-looking request — it is a 401 from a real service, months
//! later, with nothing local to reproduce it.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{channel, Sender};
use std::{fs, thread};

/// The key the fixture configures, base64 of `secret-key`.
const ACCESS_KEY_B64: &str = "c2VjcmV0LWtleQ==";

fn build_module() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/azure_mailer");
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        .status()
        .expect("run cargo for crates/modules/azure_mailer");
    assert!(status.success(), "cargo failed to build azure_mailer");
    crate_dir.join("target/release/libazure_mailer.so")
}

/// One request: read the head and the body, hand both back, answer the way
/// Azure does — 202 with an operation id.
fn handle(mut stream: TcpStream, tx: Sender<(String, String)>) {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(0) | Err(_) => break,
            Ok(_) => buf.push(byte[0]),
        }
    }
    let head = String::from_utf8_lossy(&buf).into_owned();
    let len = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    if len > 0 {
        let _ = stream.read_exact(&mut body);
    }
    let body = String::from_utf8_lossy(&body).into_owned();
    let _ = tx.send((head, body.clone()));

    let answer = r#"{"id":"op-1","status":"NotStarted"}"#;
    let _ = write!(
        stream,
        "HTTP/1.1 202 Accepted\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
        answer.len()
    );
    let _ = stream.flush();
}

fn header<'a>(head: &'a str, name: &str) -> &'a str {
    head.lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
        })
        .unwrap_or("")
}

#[test]
fn signs_and_sends_through_both_output_modes() {
    let dir = std::env::temp_dir().join(format!("code-azmail-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    fs::copy(build_module(), dir.join("azure_mailer.so")).expect("copy azure_mailer.so");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = channel();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            handle(stream, tx.clone());
        }
    });

    let program = format!(
        r#"link "azure_mailer.so" as mail

emit Config {{
    connection_string = "endpoint=http://127.0.0.1:{port}/;accesskey={ACCESS_KEY_B64}"
    from = "DoNotReply@example.com"
}} to mail get c
assert c.ok

emit Send {{
    recipient = "reader@example.com"
    cc = ["one@example.com", "two@example.com"]
    subject = "Welcome"
    html = "<p>hi</p>"
}} to mail get r
assert r ∈ SendResult
assert r.ok
| Azure's id for the send, handed back rather than dropped.
assert r.operation = "op-1"

| `from` on the Send overrides the configured default.
emit Send {{ recipient = "reader@example.com", from = "Other@example.com", text = "plain" }} to mail get o
assert o.ok
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
        assert!(ok, "the {mode} program failed");

        // Two sends per mode, and the first is the one worth reading.
        let (head, body) = rx.recv().expect("the module sent nothing");
        let _ = rx.recv().expect("the second send never arrived");

        assert!(
            head.starts_with("POST /emails:send?api-version="),
            "unexpected request line in {mode}: {}",
            head.lines().next().unwrap_or("")
        );

        // The body Azure would have received.
        let sent: serde_json::Value = serde_json::from_str(&body).expect("body is JSON");
        assert_eq!(sent["senderAddress"], "DoNotReply@example.com");
        assert_eq!(sent["content"]["subject"], "Welcome");
        assert_eq!(sent["content"]["html"], "<p>hi</p>");
        assert_eq!(sent["recipients"]["to"][0]["address"], "reader@example.com");
        assert_eq!(sent["recipients"]["cC"][1]["address"], "two@example.com");

        // The signature, recomputed from the same key. This is what a real
        // service checks, and the only thing here that a local run can prove.
        use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
        use hmac::{Hmac, Mac};
        use sha2::{Digest, Sha256};

        let date = header(&head, "x-ms-date");
        let content_hash = header(&head, "x-ms-content-sha256");
        assert_eq!(
            content_hash,
            B64.encode(Sha256::digest(body.as_bytes())),
            "the content hash does not match the body it was sent with"
        );

        let host = format!("127.0.0.1:{port}");
        let path = head
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("");
        let mut mac = <Hmac<Sha256>>::new_from_slice(b"secret-key").expect("key");
        mac.update(format!("POST\n{path}\n{date};{host};{content_hash}").as_bytes());
        let expected = B64.encode(mac.finalize().into_bytes());

        let auth = header(&head, "authorization");
        assert!(
            auth.starts_with(
                "HMAC-SHA256 SignedHeaders=x-ms-date;host;x-ms-content-sha256&Signature="
            ),
            "unexpected Authorization in {mode}: {auth}"
        );
        assert!(
            auth.ends_with(&expected),
            "the signature is not the one this key and request produce, in {mode}"
        );
    }

    let _ = fs::remove_dir_all(&dir);
}

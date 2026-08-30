//! The `mailer` module, against a real SMTP server.
//!
//! A `.code` fixture can only reach `mailer`'s error paths
//! (`tests/mailer_error_paths.code`) — a successful `Send` needs a server to
//! accept the message. This stands a minimal one up on loopback, runs a
//! program that `Config`s and `Send`s to it through **both** output modes,
//! and checks the server saw the message.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{channel, Sender};
use std::{fs, thread};

/// Build the module and return the `.so`'s path.
fn build_module() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/mailer");
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        .status()
        .expect("run cargo for crates/modules/mailer");
    assert!(status.success(), "cargo failed to build mailer");
    crate_dir.join("target/release/libmailer.so")
}

/// One connection of a plaintext SMTP server: enough of RFC 5321 to accept a
/// single message, then hand the `MAIL FROM` / `RCPT TO` / DATA body back
/// over `tx`.
fn handle_smtp(stream: TcpStream, tx: Sender<String>) {
    let mut writer = stream.try_clone().expect("clone stream");
    let mut reader = BufReader::new(stream);
    let say = |w: &mut TcpStream, line: &str| {
        let _ = write!(w, "{line}\r\n");
        let _ = w.flush();
    };
    say(&mut writer, "220 test ESMTP");

    let mut transcript = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let cmd = line.trim_end().to_string();
        let upper = cmd.to_ascii_uppercase();
        if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            say(&mut writer, "250-test");
            say(&mut writer, "250 OK");
        } else if upper.starts_with("MAIL") || upper.starts_with("RCPT") {
            transcript.push_str(&cmd);
            transcript.push('\n');
            say(&mut writer, "250 OK");
        } else if upper == "DATA" {
            say(&mut writer, "354 send it");
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                if line == ".\r\n" || line == ".\n" {
                    break;
                }
                transcript.push_str(&line);
            }
            say(&mut writer, "250 queued");
        } else if upper == "QUIT" {
            say(&mut writer, "221 bye");
            break;
        } else {
            say(&mut writer, "250 OK");
        }
    }
    let _ = tx.send(transcript);
}

/// Runs `program` (with `PORT` substituted) through both output modes
/// against a fresh one-shot SMTP server, and hands `check` each transcript.
fn with_smtp(tag: &str, program: &str, check: impl Fn(&str)) {
    let dir = std::env::temp_dir().join(format!("code-mailer-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    fs::copy(build_module(), dir.join("mailer.so")).expect("copy mailer.so");

    for mode in ["run", "build"] {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = channel();
        let server = thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                handle_smtp(stream, tx);
            }
        });

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
        assert!(ok, "{mode} mode: the program exited non-zero");

        let transcript = rx.recv().expect("server thread sent a transcript");
        let _ = server.join();
        check(&transcript);
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_configured_send_reaches_the_server() {
    let program = r#"link "mailer.so" as mail

emit Config { host = "127.0.0.1", port = PORT, from = "sender@example.com", tls = "none" } to mail get c
assert c.ok

emit Send {
    recipient = "rcpt@example.com",
    subject = "Hello",
    text = "the body",
    cc = ["carbon@example.com"]
} to mail get s
assert s ∈ SendResult
assert s.ok
"#;
    with_smtp("send", program, |transcript| {
        assert!(
            transcript.contains("MAIL FROM:<sender@example.com>"),
            "server should have seen the sender:\n{transcript}"
        );
        assert!(
            transcript.contains("RCPT TO:<rcpt@example.com>"),
            "server should have seen the recipient:\n{transcript}"
        );
        assert!(
            transcript.contains("RCPT TO:<carbon@example.com>"),
            "cc is a recipient at the SMTP layer:\n{transcript}"
        );
        assert!(
            transcript.contains("Subject: Hello") && transcript.contains("the body"),
            "the message body should carry the subject and text:\n{transcript}"
        );
    });
}

//! The `ntfy` module against a server — a few lines of HTTP written here,
//! since a test that needs ntfy.sh is a test that fails on a train. The
//! server records the one request and answers as ntfy does, with the
//! message's id as JSON.
//!
//! What the fixture proves: the notification is a `POST` to
//! `<url>/<topic>`, the body is the message, and title, priority, tags,
//! click and auth ride as the headers ntfy reads; a topic given in the
//! particle wins over Config's; the id comes back; and a server that
//! refuses is an `Exception` carrying its words.
//!
//! Both output modes, because a module is exactly where `code run` and
//! `code build` could differ.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{fs, thread};

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

fn workspace(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("code-ntfy-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    fs::copy(build_module("ntfy"), dir.join("ntfy.so")).expect("copy ntfy.so");
    dir
}

// --- the server -----------------------------------------------------------

/// One request as the server saw it.
#[derive(Clone, Debug, Default)]
struct Seen {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn serve(mut s: TcpStream, log: Arc<Mutex<Vec<Seen>>>) {
    let mut reader = BufReader::new(s.try_clone().expect("clone"));
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() || line.is_empty() {
        return;
    }
    let mut parts = line.split_whitespace();
    let mut seen = Seen {
        method: parts.next().unwrap_or("").to_string(),
        path: parts.next().unwrap_or("").to_string(),
        ..Default::default()
    };
    let mut length = 0usize;
    loop {
        line.clear();
        if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            let (k, v) = (k.trim().to_string(), v.trim().to_string());
            if k.eq_ignore_ascii_case("content-length") {
                length = v.parse().unwrap_or(0);
            }
            seen.headers.push((k, v));
        }
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).expect("body");
    seen.body = String::from_utf8_lossy(&body).into_owned();

    // The topic `refused` is the server saying no; anything else is taken.
    let (status, answer) = if seen.path == "/refused" {
        (
            "403 Forbidden",
            r#"{"code":40301,"http":403,"error":"forbidden"}"#,
        )
    } else {
        (
            "200 OK",
            r#"{"id":"abc123","time":1,"event":"message","topic":"t","message":"m"}"#,
        )
    };
    log.lock().unwrap().push(seen);
    let _ = write!(
        s,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
        answer.len()
    );
    let _ = s.flush();
}

fn server() -> (u16, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("port").port();
    let log = Arc::new(Mutex::new(Vec::new()));
    let seen = log.clone();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let log = seen.clone();
            thread::spawn(move || serve(stream, log));
        }
    });
    (port, log)
}

fn fixture(port: u16) -> String {
    r#"link "ntfy.so" as push

emit Config { topic = "house", url = "http://127.0.0.1:PORT/", token = "tk_secret" } to push get c
assert c.ok

| The whole shape at once.
emit Notify { message = "Bedroom sensor battery at 12%", title = "Change a battery", priority = "high", tags = ["battery", "warning"], click = "https://apps.codeloves.me/home" } to push get sent
assert sent ∈ Notified
assert sent.ok
assert sent.id = "abc123"

| A topic in the particle wins, a numeric priority is passed as a number,
| a single tag is fine, and markdown asks for it.
emit Notify { message = "*bold*", topic = "other", priority = 5, tags = "tada", markdown = true } to push get again
assert again.ok

| The server saying no is an Exception carrying its words.
emit Notify { message = "hi", topic = "refused" } to push get no
assert no ∈ Exception
assert no.source = "ntfy"
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
                panic!("{mode}: the program never finished");
            }
        }
    }
}

#[test]
fn a_notification_is_one_post_with_its_headers() {
    let (port, log) = server();
    let dir = workspace("post");
    let source = dir.join("program.code");
    fs::write(&source, fixture(port)).expect("write fixture");
    for mode in ["run", "build"] {
        log.lock().unwrap().clear();
        assert!(
            run_to_end(&dir, mode, &source),
            "{mode}: the fixture failed"
        );

        let seen = log.lock().unwrap().clone();
        assert_eq!(seen.len(), 3, "{mode}: three requests, got {seen:?}");

        let first = &seen[0];
        assert_eq!(first.method, "POST");
        assert_eq!(first.path, "/house");
        assert_eq!(first.body, "Bedroom sensor battery at 12%");
        assert_eq!(first.header("Title"), Some("Change a battery"));
        assert_eq!(first.header("Priority"), Some("high"));
        assert_eq!(first.header("Tags"), Some("battery,warning"));
        assert_eq!(
            first.header("Click"),
            Some("https://apps.codeloves.me/home")
        );
        assert_eq!(first.header("Authorization"), Some("Bearer tk_secret"));
        assert!(first.header("Markdown").is_none());

        let second = &seen[1];
        assert_eq!(second.path, "/other", "{mode}: the particle's topic wins");
        assert_eq!(second.header("Priority"), Some("5"));
        assert_eq!(second.header("Tags"), Some("tada"));
        assert_eq!(second.header("Markdown"), Some("yes"));
        assert!(second.header("Title").is_none());

        assert_eq!(seen[2].path, "/refused");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn basic_auth_is_the_pair_encoded() {
    let (port, log) = server();
    let dir = workspace("basic");
    let source = dir.join("program.code");
    fs::write(
        &source,
        format!(
            "link \"ntfy.so\" as push\nemit Config {{ topic = \"t\", url = \"http://127.0.0.1:{port}\", username = \"me\", password = \"pw\" }} to push get c\nassert c.ok\nemit Notify {{ message = \"x\" }} to push get n\nassert n.ok\n"
        ),
    )
    .expect("write fixture");
    assert!(run_to_end(&dir, "run", &source), "the fixture failed");
    let seen = log.lock().unwrap().clone();
    assert_eq!(seen[0].header("Authorization"), Some("Basic bWU6cHc="));
    let _ = fs::remove_dir_all(&dir);
}

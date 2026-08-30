//! The `localai` module, against a fake OpenAI-compatible endpoint.
//!
//! The guard paths are pure and covered by `tests/localai_error_paths.code`.
//! `Chat`, `ChatJson` and `Transcribe` each need a server on the other end;
//! this stands a minimal one up on loopback — enough of
//! `/v1/chat/completions` and `/v1/audio/transcriptions` for one round trip
//! each — and drives the module through both output modes.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{fs, thread};

fn build_module() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/localai");
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        .status()
        .expect("run cargo for crates/modules/localai");
    assert!(status.success(), "cargo failed to build localai");
    crate_dir.join("target/release/liblocalai.so")
}

/// The reasoning-model reply: a `<think>` block, then a fenced JSON object.
/// Plain `Chat` keeps the fence; `ChatJson` strips it and canonicalises.
const REPLY: &str =
    "<think>weighing it up</think>\\n```json\\n{\\\"answer\\\": 42,  \\\"unit\\\": \\\"pt\\\"}\\n```";

fn read_request(stream: &mut TcpStream) -> (String, String) {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(0) | Err(_) => break,
            Ok(_) => buf.push(byte[0]),
        }
    }
    let head = String::from_utf8_lossy(&buf).into_owned();
    let mut parts = head.lines().next().unwrap_or_default().split_whitespace();
    let _method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default().to_string();

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
    (path, String::from_utf8_lossy(&body).into_owned())
}

fn respond(stream: &mut TcpStream, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

fn fake_openai(listener: TcpListener) {
    for incoming in listener.incoming() {
        let Ok(mut stream) = incoming else { continue };
        let (path, req_body) = read_request(&mut stream);

        if path == "/v1/audio/transcriptions" {
            respond(&mut stream, r#"{"text":"  the transcript  "}"#);
            continue;
        }
        if path == "/v1/chat/completions" {
            // A multi-turn request carries a prior assistant turn; answer it
            // distinctly so the test can prove `messages` reached the wire.
            let content = if req_body.contains(r#""role":"assistant""#) {
                "multi-turn ok".to_string()
            } else {
                REPLY.to_string()
            };
            respond(
                &mut stream,
                &format!(r#"{{"choices":[{{"message":{{"content":"{content}"}}}}]}}"#),
            );
            continue;
        }
        respond(&mut stream, r#"{"error":"unexpected path"}"#);
    }
}

#[test]
fn chat_chatjson_and_transcribe_round_trip() {
    let dir = std::env::temp_dir().join(format!("code-localai-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    fs::copy(build_module(), dir.join("localai.so")).expect("copy localai.so");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || fake_openai(listener));

    let program = format!(
        r#"link "localai.so" as ai

emit Config {{ endpoint = "http://127.0.0.1:{port}", model = "test-model" }} to ai get c
assert c.ok

emit Chat {{ system = "be terse", user = "how many?" }} to ai get plain
assert plain ∈ ChatResult
assert plain.content = "```json\n{{\"answer\": 42,  \"unit\": \"pt\"}}\n```"

emit ChatJson {{ user = "how many?" }} to ai get structured
assert structured.content = "{{\"answer\":42,\"unit\":\"pt\"}}"

emit Chat {{ messages = [
    {{ role = "system", content = "be terse" }},
    {{ role = "user", content = "hi" }},
    {{ role = "assistant", content = "hello" }},
    {{ role = "user", content = "still there?" }}
] }} to ai get multi
assert multi.content = "multi-turn ok"

emit Transcribe {{ audio_base64 = "aGVsbG8=", language = "en" }} to ai get t
assert t ∈ TranscribeResult
assert t.text = "the transcript"
assert t.language = "en"

emit TranscribeWithOptions {{ audio_base64 = "aGVsbG8=" }} to ai get t2
assert t2.text = "the transcript"
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
        assert!(ok, "{mode} mode: the localai round trip exited non-zero");
    }

    let _ = fs::remove_dir_all(&dir);
}

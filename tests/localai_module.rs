//! The `localai` module, against a fake OpenAI-compatible endpoint.
//!
//! The guard paths are pure and covered by `tests/localai_error_paths.code`.
//! `Chat`, `ChatJson` and `Transcribe` each need a server on the other end;
//! this stands a minimal one up on loopback — enough of
//! `/v1/chat/completions` and `/v1/audio/transcriptions` for one round trip
//! each — and drives the module through both output modes.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

#[path = "support/modules.rs"]
mod modules;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::{fs, thread};

fn build_module() -> PathBuf {
    modules::build_so("localai")
}

/// The reasoning-model reply: a `<think>` block, then a fenced JSON object.
/// Plain `Chat` keeps the fence; `ChatJson` strips it and canonicalises.
const REPLY: &str =
    "<think>weighing it up</think>\\n```json\\n{\\\"answer\\\": 42,  \\\"unit\\\": \\\"pt\\\"}\\n```";

/// What a LocalAI with `API_KEY` set wants on every request. The fake below
/// refuses without it, so the test proves the key travels — for the JSON
/// post and the multipart one alike.
const API_KEY: &str = "test-key-7f";

fn read_request(stream: &mut TcpStream) -> (String, String, bool) {
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
    let authorized = head.lines().any(|l| {
        l.split_once(':').is_some_and(|(k, v)| {
            k.trim().eq_ignore_ascii_case("authorization")
                && v.trim() == format!("Bearer {API_KEY}")
        })
    });
    (
        path,
        String::from_utf8_lossy(&body).into_owned(),
        authorized,
    )
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
        let (path, req_body, authorized) = read_request(&mut stream);

        if !authorized {
            let body = r#"{"error":{"message":"invalid api key"}}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.flush();
            continue;
        }
        if path == "/v1/audio/transcriptions" {
            respond(&mut stream, r#"{"text":"  the transcript  "}"#);
            continue;
        }
        if path == "/v1/chat/completions" {
            // A multi-turn request carries a prior assistant turn; answer it
            // distinctly so the test can prove `messages` reached the wire.
            let request: serde_json::Value = serde_json::from_str(&req_body).unwrap();
            if request["model"] == "vision-error" {
                let reflected = request.to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    reflected.len(),
                    reflected
                );
                let _ = stream.flush();
                continue;
            }
            // Tools: a request offering them is answered with a call; the
            // follow-up carrying the call and its result gets plain text.
            if request["model"] == "tools-test" {
                let offered = request["tools"][0]["function"]["name"] == "run_step"
                    && request["tool_choice"] == "auto";
                let followed = request["messages"][2]["tool_calls"][0]["id"] == "call-1"
                    && request["messages"][3]["role"] == "tool"
                    && request["messages"][3]["tool_call_id"] == "call-1";
                let body = if followed {
                    r#"{"choices":[{"message":{"content":"listed","tool_calls":null}}]}"#
                        .to_string()
                } else if offered {
                    r#"{"choices":[{"message":{"content":null,"tool_calls":[{"id":"call-1","type":"function","function":{"name":"run_step","arguments":"{\"command\":\"ls ~/Music\"}"}}]}}]}"#.to_string()
                } else {
                    r#"{"choices":[{"message":{"content":"bad tools request"}}]}"#.to_string()
                };
                respond(&mut stream, &body);
                continue;
            }
            let content = if request["model"] == "vision-test" {
                let valid = request["reasoning_effort"] == "low"
                    && request["chat_template_kwargs"]["enable_thinking"] == true
                    && request["messages"][1]["content"][0]["type"] == "text"
                    && request["messages"][1]["content"][1]["type"] == "image_url"
                    && request["messages"][1]["content"][1]["image_url"]["url"]
                        .as_str()
                        .is_some_and(|s| s.starts_with("data:image/png;base64,"));
                if valid {
                    "vision thinking ok".to_string()
                } else {
                    "bad vision request".to_string()
                }
            } else if request["model"] == "thinking-off" {
                if request["reasoning_effort"] == "none"
                    && request["chat_template_kwargs"]["enable_thinking"] == false
                {
                    "thinking disabled".to_string()
                } else {
                    "bad thinking request".to_string()
                }
            } else if req_body.contains(r#""role":"assistant""#) {
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

| Without the key the server refuses, and the refusal is an Exception here.
emit Config {{ endpoint = "http://127.0.0.1:{port}", model = "test-model" }} to ai get unkeyed
assert unkeyed.ok
emit Chat {{ user = "anyone there?" }} to ai get refused
assert refused ∈ Exception

emit Config {{ endpoint = "http://127.0.0.1:{port}", model = "test-model", api_key = "{API_KEY}" }} to ai get c
assert c.ok

emit Chat {{ system = "be terse", user = "how many?" }} to ai get plain
assert plain ∈ ChatResult
assert plain.content = "```json\n{{\"answer\": 42,  \"unit\": \"pt\"}}\n```"

emit ChatJson {{ user = "how many?" }} to ai get structured
assert structured.content = "{{\"answer\":42,\"unit\":\"pt\"}}"

emit Chat {{ messages = [
    {{ role = "system", content = "be terse" }}
    {{ role = "user", content = "hi" }}
    {{ role = "assistant", content = "hello" }}
    {{ role = "user", content = "still there?" }}
] }} to ai get multi
assert multi.content = "multi-turn ok"

| A decoded attachment becomes a multimodal user message; settings reach the wire.
images = [{{ mime_type = "image/png", data_base64 = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFklEQVR4nGP8z8DAwMDAxMDAwMDAAAANHQEDasKb6QAAAABJRU5ErkJggg==" }}]
emit ValidateImages {{ images = images }} to ai get checked
assert checked ∈ ImagesValidated
assert checked.value = 1
emit Chat {{ model = "vision-test", thinking = "low", system = "look", user = "what color?", images = images }} to ai get vision
assert vision.content = "vision thinking ok"
emit Chat {{ model = "vision-error", user = "look", images = images }} to ai get image_error
assert image_error ∈ Exception
assert image_error.message = "chat rejected: HTTP 400 (image response details omitted)"
emit Chat {{ model = "thinking-off", thinking = "off", user = "hi" }} to ai get off
assert off.content = "thinking disabled"
emit Chat {{ user = "hi", thinking = "lots" }} to ai get invalid_thinking
assert invalid_thinking ∈ Exception
emit ValidateImages {{ images = [{{ mime_type = "image/jpeg", data_base64 = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFklEQVR4nGP8z8DAwMDAxMDAwMDAAAANHQEDasKb6QAAAABJRU5ErkJggg==" }}] }} to ai get mismatch
assert mismatch ∈ Exception
emit ValidateImages {{ images = [{{ mime_type = "image/png", data_base64 = "aGVsbG8=" }}] }} to ai get corrupt
assert corrupt ∈ Exception
emit ValidateImages {{ images = [{{ mime_type = "image/png", data_base64 = "%%%" }}] }} to ai get bad_base64
assert bad_base64 ∈ Exception
emit ValidateImages {{ images = images + images + images + images }} to ai get too_many
assert too_many ∈ Exception

| Tools: the model's call comes back as tool_calls, arguments as its text;
| the call and its result go back as assistant/tool messages.
tools = [{{ type = "function", function = {{ name = "run_step", description = "run", parameters = {{ type = "object", properties = {{ command = {{ type = "string" }} }}, required = ["command"] }} }} }}]
emit ChatJson {{ model = "tools-test", user = "ls in music", tools = tools, tool_choice = "auto" }} to ai get called
assert called ∈ ChatResult
assert called.content = ""
assert called.tool_calls[0].id = "call-1"
assert called.tool_calls[0].name = "run_step"
assert called.tool_calls[0].arguments = "{{\"command\":\"ls ~/Music\"}}"
emit Chat {{ model = "tools-test", tools = tools, messages = [
    {{ role = "system", content = "act" }}
    {{ role = "user", content = "ls in music" }}
    {{ role = "assistant", tool_calls = [{{ id = "call-1", type = "function", function = {{ name = "run_step", arguments = "{{}}" }} }}] }}
    {{ role = "tool", tool_call_id = "call-1", content = "a.mp3" }}
] }} to ai get followed
assert followed.content = "listed"
emit Chat {{ user = "hi", tools = "nope" }} to ai get bad_tools
assert bad_tools ∈ Exception

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

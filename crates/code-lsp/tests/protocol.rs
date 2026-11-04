//! Exercise the real stdio server, not a mock analysis callback.
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{json, Value};
use tower_lsp::lsp_types::Url;

struct Client {
    child: Child,
    input: ChildStdin,
    messages: Receiver<Value>,
}

impl Client {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_code-lsp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (sender, messages) = mpsc::channel();
        std::thread::spawn(move || {
            let mut output = BufReader::new(output);
            loop {
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    if output.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.strip_prefix("Content-Length: ") {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                if output.read_exact(&mut body).is_err() {
                    return;
                }
                if sender.send(serde_json::from_slice(&body).unwrap()).is_err() {
                    return;
                }
            }
        });
        Self {
            child,
            input,
            messages,
        }
    }

    fn send(&mut self, message: Value) {
        let body = serde_json::to_vec(&message).unwrap();
        write!(self.input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        self.input.write_all(&body).unwrap();
        self.input.flush().unwrap();
    }

    fn until(&self, predicate: impl Fn(&Value) -> bool) -> Value {
        let mut seen = Vec::new();
        loop {
            let message = self
                .messages
                .recv_timeout(Duration::from_secs(15))
                .unwrap_or_else(|error| panic!("server response: {error}; received {seen:?}"));
            if predicate(&message) {
                return message;
            }
            seen.push(message);
        }
    }

    fn diagnostics(&self, uri: &str, version: Option<i64>, contains: Option<&str>) -> Vec<Value> {
        let notification = self.until(|value| {
            value["method"] == "textDocument/publishDiagnostics"
                && value["params"]["uri"] == uri
                && value["params"]["version"].as_i64() == version
                && value["params"]["diagnostics"]
                    .as_array()
                    .is_some_and(|diagnostics| {
                        contains.map_or(diagnostics.is_empty(), |part| {
                            diagnostics
                                .iter()
                                .any(|d| d["message"].as_str().unwrap().contains(part))
                        })
                    })
        });
        notification["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn open(&mut self, uri: &str, text: &str) {
        self.send(json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"languageId":"code","version":1,"text":text}}}));
    }

    fn change(&mut self, uri: &str, version: i64, text: &str) {
        self.send(json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":uri,"version":version},"contentChanges":[{"text":text}]}}));
    }

    fn close(&mut self, uri: &str) {
        self.send(json!({"jsonrpc":"2.0","method":"textDocument/didClose","params":{"textDocument":{"uri":uri}}}));
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("code-lsp-protocol-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn uri(&self, file: &str) -> String {
        Url::from_file_path(self.0.join(file)).unwrap().to_string()
    }
    fn write(&self, file: &str, text: &str) {
        std::fs::write(self.0.join(file), text).unwrap();
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn editor_checks_unsaved_programs_without_running_them() {
    let project = Project::new();
    let mut client = Client::start();
    client.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}));
    let init = client.until(|v| v["id"] == 1);
    assert_eq!(
        init["result"]["capabilities"]["documentFormattingProvider"],
        true
    );
    assert!(init["result"]["capabilities"]["semanticTokensProvider"].is_object());
    assert_eq!(
        init["result"]["capabilities"]["textDocumentSync"]["change"],
        1
    );
    client.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));

    let file = project.uri("scratch.code");
    client.open(&file, "assert missing_name = 1\n");
    let errors = client.diagnostics(&file, Some(1), Some("undefined variable 'missing_name'"));
    assert_eq!(errors[0]["range"]["start"]["line"], 0);
    client.change(&file, 2, "assert 1 = 1\n");
    client.diagnostics(&file, Some(2), None);

    client.change(&file, 3, "Greet { who ∈ String } => return Greeting { value = who }\nport ∈ Number = \"oops\"\nemit Grete { who = \"Ada\" } to this\nemit Greet { woh = \"Ada\" } to this\n");
    let errors = client.diagnostics(&file, Some(3), Some("expects Number"));
    for code in ["type-mismatch", "unknown-handler", "unknown-field"] {
        assert!(errors.iter().any(|d| d["code"] == code), "{errors:?}");
    }
    assert!(errors
        .iter()
        .any(|d| d["message"].as_str().unwrap().contains("try 'Greet'")));

    let unicode = "face = \"🙂\", port ∈ Number = \"oops\"\n";
    client.change(&file, 4, unicode);
    let errors = client.diagnostics(&file, Some(4), Some("expects Number"));
    assert_eq!(
        errors[0]["range"]["start"]["character"],
        "face = \"🙂\", ".encode_utf16().count()
    );
    client.change(&file, 5, "x=1\n");
    client.diagnostics(&file, Some(5), None);
    // A delayed older notification must not overwrite the newer buffer.
    client.change(&file, 4, "assert stale_name\n");
    client.send(json!({"jsonrpc":"2.0","id":2,"method":"textDocument/formatting","params":{"textDocument":{"uri":file},"options":{"tabSize":4,"insertSpaces":true}}}));
    let formatting = client.until(|v| v["id"] == 2);
    assert_eq!(formatting["result"][0]["newText"], "x = 1\n");
    client.send(json!({"jsonrpc":"2.0","id":3,"method":"textDocument/semanticTokens/full","params":{"textDocument":{"uri":file}}}));
    assert!(!client.until(|v| v["id"] == 3)["result"]["data"]
        .as_array()
        .unwrap()
        .is_empty());
    client.close(&file);
    client.diagnostics(&file, None, None);

    project.write(
        "helper.code",
        "private = 9\nRespond => return Reply { value = 1 }\n",
    );
    let view_text = "Render =>\n    emit Respond to this get reply\n    return reply\n";
    project.write("view.code", view_text);
    project.write("opaque.a", "not an archive; inspecting it would fail");
    let main_text = "link \"helper.code\"\nlink \"view.code\"\nlink \"opaque.a\" as plugin\nemit Work to plugin\nassert false\nloop\n    continue\n";
    project.write("main.code", main_text);
    let main = project.uri("main.code");
    let view = project.uri("view.code");
    let helper = project.uri("helper.code");
    client.open(&main, main_text);
    // The assertion, endless loop and fake archive prove this does not run or
    // load native code. Static checks still finish and keep dynamic sends valid.
    client.diagnostics(&main, Some(1), None);
    client.open(&view, view_text);
    client.diagnostics(&view, Some(1), None);
    client.change(&view, 2, "Render => return Reply { value = private }\n");
    let errors = client.diagnostics(&view, Some(2), Some("undefined variable 'private'"));
    assert!(errors
        .iter()
        .any(|d| d["message"].as_str().unwrap().contains("helper.code")));
    client.change(&view, 3, view_text);
    client.diagnostics(&view, Some(3), None);
    client.open(&helper, "Other => return Reply { value = 2 }\n");
    client.diagnostics(&view, Some(3), Some("no handler named 'Respond'"));
    client.close(&helper); // Reverts to disk's Respond handler for the importer.
    client.diagnostics(&view, Some(3), None);
    client.change(&view, 4, "fn example()\n");
    client.diagnostics(&view, Some(4), Some("there are no functions"));
    client.change(&view, 5, view_text);
    client.diagnostics(&view, Some(5), None);

    // A change in a closed dependency is picked up when the entry is saved.
    project.write("helper.code", "Other => return Reply\n");
    client.send(json!({"jsonrpc":"2.0","method":"textDocument/didSave","params":{"textDocument":{"uri":main}}}));
    client.diagnostics(&view, Some(5), Some("no handler named 'Respond'"));
    project.write("helper.code", "Respond => return Reply\n");
    client.close(&main);
    client.diagnostics(&view, Some(5), None);
    client.close(&view);
    client.diagnostics(&view, None, None);

    let untitled = "untitled:code-lsp-new-file";
    client.open(untitled, "assert unbound = 1\n");
    client.diagnostics(untitled, Some(1), Some("undefined variable 'unbound'"));
    client.close(untitled);
    client.diagnostics(untitled, None, None);
    client.send(json!({"jsonrpc":"2.0","id":4,"method":"shutdown","params":null}));
    client.until(|v| v["id"] == 4);
}

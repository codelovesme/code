//! `clipboard`'s page half puts text on the reader's clipboard: asked, the
//! text goes; without a clipboard the answer says so; a browser that
//! refuses afterwards fires its refusal as a particle of its own. Run under
//! node against a stand-in `navigator` — the half asks it for nothing a
//! fake cannot answer.

use std::fs;
use std::path::Path;
use std::process::Command;

fn tool_exists(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn a_copy_reaches_the_clipboard_or_says_why_not() {
    if !tool_exists("node") {
        eprintln!("skipped: needs node");
        return;
    }
    let half = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/clipboard/page.mjs");
    let dir = std::env::temp_dir().join(format!("code-clipboard-copy-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create the probe dir");
    let probe = dir.join("probe.mjs");
    fs::write(
        &probe,
        format!(
            r#"import {{ readFileSync }} from "node:fs";
const half = new Function("return (" + readFileSync({half:?}, "utf8") + ")")();
const fired = [];
const [, clip] = half({{ fire: (p) => fired.push(p) }});
const check = (what, got, want) => {{
  if (JSON.stringify(got) !== JSON.stringify(want)) {{
    console.error(`${{what}}: got ${{JSON.stringify(got)}}, wanted ${{JSON.stringify(want)}}`);
    process.exit(1);
  }}
}};

let clipboard = undefined;
Object.defineProperty(globalThis, "navigator", {{ get: () => ({{ clipboard }}), configurable: true }});
check("a copy with no clipboard claimed success", clip({{ _class: "Copy", text: "hi" }}), {{ _class: "CopyResult", ok: false }});
let copied = null;
clipboard = {{ writeText: (t) => {{ copied = t; return Promise.resolve(); }} }};
check("a copy did not answer success", clip({{ _class: "Copy", text: "hello" }}), {{ _class: "CopyResult", ok: true }});
check("the text did not reach the clipboard", copied, "hello");
clipboard = {{ writeText: () => Promise.reject(new Error("not allowed")) }};
clip({{ _class: "Copy", text: "no" }});
await new Promise((r) => setTimeout(r, 0));
check("a refused copy did not say why", fired, [{{ _class: "CopyFailed", reason: "not allowed" }}]);
check("a class it does not handle was not null", clip({{ _class: "Paste" }}), null);
"#
        ),
    )
    .expect("write the probe");

    let output = Command::new("node")
        .arg(&probe)
        .output()
        .expect("run the probe under node");
    assert!(
        output.status.success(),
        "clipboard's copy did not hold: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

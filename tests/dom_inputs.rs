//! `dom`'s page half carries a chosen file into the program, says which key
//! a key event was, and focuses the node a render marked: a file box's
//! `value` is a path the browser made up, so on `change` the bytes are read
//! and sent as `file`, and a file too large to carry is refused with a word
//! rather than dropped. Run under node against a document of the test's own
//! — the half asks the document for nothing a fake cannot answer.

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
fn a_file_box_sends_the_bytes_or_says_why_not() {
    if !tool_exists("node") {
        eprintln!("skipped: needs node");
        return;
    }
    let half = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/dom/page.mjs");
    let dir = std::env::temp_dir().join(format!("code-dom-file-box-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create the probe dir");
    let probe = dir.join("probe.mjs");
    fs::write(
        &probe,
        format!(
            r#"import {{ readFileSync }} from "node:fs";
const half = new Function("return (" + readFileSync({half:?}, "utf8") + ")")();

class El {{
  constructor(tag) {{
    this.tagName = tag.toUpperCase(); this.nodeType = 1; this.children = []; this.parentNode = null;
    this.attrs = {{}}; this.listeners = {{}}; this.value = ""; this.files = [];
  }}
  addEventListener(name, fn) {{ (this.listeners[name] ||= []).push(fn); }}
  setAttribute(k, v) {{ this.attrs[k] = String(v); }}
  hasAttribute(k) {{ return k in this.attrs; }}
  getAttribute(k) {{ return this.attrs[k]; }}
  appendChild(c) {{ c.parentNode = this; this.children.push(c); return c; }}
  replaceChildren(c) {{ this.children = []; this.appendChild(c); }}
  send(name, e) {{ for (const fn of this.listeners[name] || []) fn({{ target: this, ...e }}); }}
  focus() {{ this.focused = true; doc.activeElement = this; }}
  contains(other) {{ for (let n = other; n; n = n.parentNode) if (n === this) return true; return false; }}
  setSelectionRange(a, b) {{ this.selectionStart = a; this.selectionEnd = b; }}
  querySelector(sel) {{
    if (sel !== "[autofocus]") return null;
    for (const c of this.children) {{
      if (c.nodeType !== 1) continue;
      if ("autofocus" in c.attrs) return c;
      const deeper = c.querySelector(sel);
      if (deeper) return deeper;
    }}
    return null;
  }}
}}
// The browser's reader, answering at once with what the fake file says it is.
globalThis.FileReader = class {{
  readAsDataURL(f) {{
    if (f.broken) {{ this.onerror && this.onerror(); return; }}
    this.result = "data:" + f.type + ";base64," + f.b64;
    this.onload && this.onload();
  }}
}};
const body = new El("body");
const doc = {{
  createElement: (t) => new El(t),
  createTextNode: (s) => ({{ nodeType: 3, text: String(s) }}),
  querySelector: (sel) => (sel === "body" ? body : null),
  getElementById: () => null,
  head: body,
  activeElement: null,
}};
const fired = [];
const [, dom] = half({{ doc, fire: (p) => fired.push(p) }});
const check = (what, got, want) => {{
  if (JSON.stringify(got) !== JSON.stringify(want)) throw new Error(`${{what}}: got ${{JSON.stringify(got)}}, wanted ${{JSON.stringify(want)}}`);
}};

dom({{ _class: "Render", into: "body", tree: {{ tag: "div", children: [
  {{ tag: "input", attrs: {{ type: "file" }}, on: {{ change: {{ _class: "Chosen", slot: 2 }} }} }},
  {{ tag: "input", attrs: {{ type: "text" }}, on: {{ change: "Typed" }} }},
] }} }});
const [box, text] = body.children[0].children;

// The bytes, the name as the value, and the application's own fields kept.
box.value = "C:\\fakepath\\notes.txt";
box.files = [{{ name: "notes.txt", type: "text/plain", size: 5, b64: "aGVsbG8=" }}];
box.send("change", {{}});
check("a chosen file was not carried", fired, [{{ _class: "Chosen", slot: 2, value: "notes.txt",
  file: {{ name: "notes.txt", type: "text/plain", size: 5, data_base64: "aGVsbG8=" }} }}]);
fired.length = 0;

// Too large: told, with nothing to carry.
box.files = [{{ name: "film.mov", type: "video/quicktime", size: 40 * 1024 * 1024, b64: "" }}];
box.send("change", {{}});
check("a large file was not refused with a word", fired, [{{ _class: "Chosen", slot: 2, value: "film.mov",
  file: {{ name: "film.mov", type: "video/quicktime", size: 41943040, data_base64: "", refused: "larger than 12 MB" }} }}]);
fired.length = 0;

// One the browser could not read: the same, in its own words.
box.files = [{{ name: "gone.bin", type: "", size: 3, broken: true }}];
box.send("change", {{}});
check("an unreadable file was not said", fired, [{{ _class: "Chosen", slot: 2, value: "gone.bin",
  file: {{ name: "gone.bin", type: "", size: 3, data_base64: "", refused: "could not be read" }} }}]);
fired.length = 0;

// A change with no file chosen says nothing; a text box is untouched.
box.files = [];
box.send("change", {{}});
check("an empty file box sent something", fired, []);
text.value = "hi";
text.send("change", {{}});
check("a text box stopped carrying its text", fired, [{{ _class: "Typed", value: "hi" }}]);
fired.length = 0;

// A key event says which key; one the application already named is kept.
dom({{ _class: "Render", into: "body", tree: {{ tag: "div", children: [
  {{ tag: "input", attrs: {{ type: "text", autofocus: "" }}, on: {{ keydown: "Pressed" }} }},
  {{ tag: "div", on: {{ keyup: {{ _class: "Told", key: "mine" }} }} }},
] }} }});
const [typing, told] = body.children[0].children;
check("the autofocus node was not focused on render", typing.focused, true);
typing.value = "x";
typing.send("keydown", {{ key: "Escape" }});
told.send("keyup", {{ key: "Enter" }});
check("a key event did not say its key", fired, [{{ _class: "Pressed", value: "x", key: "Escape" }}, {{ _class: "Told", key: "mine" }}]);

// The caret survives a render: the box at the same place in the new tree,
// of the same kind, is focused again with its selection; a different kind
// of node there is not.
const twice = {{ tag: "div", children: [ {{ tag: "p" }}, {{ tag: "input", attrs: {{ type: "text" }} }} ] }};
dom({{ _class: "Render", into: "body", tree: twice }});
const first = body.children[0].children[1];
first.focus(); first.selectionStart = 2; first.selectionEnd = 3;
dom({{ _class: "Render", into: "body", tree: twice }});
const second = body.children[0].children[1];
check("the caret did not come back after a render", [second !== first, second.focused, second.selectionStart, second.selectionEnd], [true, true, 2, 3]);
dom({{ _class: "Render", into: "body", tree: {{ tag: "div", children: [ {{ tag: "p" }}, {{ tag: "button" }} ] }} }});
check("a different node at the caret's place was focused", body.children[0].children[1].focused, undefined);
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
        "dom's file box did not hold: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

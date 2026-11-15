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
let downloaded = null;

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
  removeChild(c) {{ this.children = this.children.filter(child => child !== c); c.parentNode = null; return c; }}
  click() {{ downloaded = this; }}
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
  body,
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
  file: {{ name: "film.mov", type: "video/quicktime", size: 41943040, data_base64: "", refused: "larger than 12 MB", handle: "file-1" }} }}]);
fired.length = 0;

// One the browser could not read: the same, in its own words.
box.files = [{{ name: "gone.bin", type: "", size: 3, broken: true }}];
box.send("change", {{}});
check("an unreadable file was not said", fired, [{{ _class: "Chosen", slot: 2, value: "gone.bin",
  file: {{ name: "gone.bin", type: "", size: 3, data_base64: "", refused: "could not be read" }} }}]);
fired.length = 0;

// Several chosen together (`multiple`): one event each, in their order, a
// too-large one refused on its own without stopping the rest.
box.files = [
  {{ name: "a.jpg", type: "image/png", size: 4, b64: "QUFB" }},
  {{ name: "big.png", type: "image/png", size: 40 * 1024 * 1024, b64: "" }},
  {{ name: "c.png", type: "image/png", size: 4, b64: "Q0ND" }},
];
box.send("change", {{}});
check("several files were not each carried, in order", fired.map((p) => [p.value, p.file.data_base64, p.file.refused || ""]),
  [["a.jpg", "QUFB", ""], ["big.png", "", "larger than 12 MB"], ["c.png", "Q0ND", ""]]);
fired.length = 0;

// A change with no file chosen says nothing; a text box is untouched.
box.files = [];
box.send("change", {{}});
check("an empty file box sent something", fired, []);
text.value = "hi";
text.send("change", {{}});
check("a text box stopped carrying its text", fired, [{{ _class: "Typed", value: "hi" }}]);
fired.length = 0;

// A paste that carries a file sends it like a file box; pasted text does not
// fire (the box's own input says it), and the image is not pasted as text.
dom({{ _class: "Render", into: "body", tree: {{ tag: "textarea", on: {{ paste: "Pasted" }} }} }});
const area = body.children[0];
let stopped = 0;
area.send("paste", {{ preventDefault() {{ stopped++; }}, clipboardData: {{ files: [], items: [
  {{ kind: "string", getAsFile: () => null }},
  {{ kind: "file", getAsFile: () => ({{ name: "image.png", type: "image/png", size: 4, b64: "iVBO" }}) }},
] }} }});
check("a pasted image was not carried", fired, [{{ _class: "Pasted", value: "image.png",
  file: {{ name: "image.png", type: "image/png", size: 4, data_base64: "iVBO" }} }}]);
check("a pasted image was also pasted as text", stopped, 1);
fired.length = 0;
area.send("paste", {{ preventDefault() {{ stopped++; }}, clipboardData: {{ files: [], items: [{{ kind: "string", getAsFile: () => null }}] }} }});
check("pasted text fired a paste", fired, []);
check("pasted text was stopped", stopped, 1);

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
fired.length = 0;
// Held modifiers come along, and only those held.
typing.send("keydown", {{ key: "Enter", shiftKey: true }});
typing.send("keydown", {{ key: "s", ctrlKey: true, metaKey: false }});
check("a key event did not say which modifiers were held", fired, [{{ _class: "Pressed", value: "x", key: "Enter", shift: true }}, {{ _class: "Pressed", value: "x", key: "s", ctrl: true }}]);
fired.length = 0;

// The caret survives a render: the box at the same place in the new tree,
// of the same kind, stays alive and keeps its selection; a different kind
// of node there is replaced and is not focused.
const twice = {{ tag: "div", children: [ {{ tag: "p" }}, {{ tag: "input", attrs: {{ type: "text" }} }} ] }};
dom({{ _class: "Render", into: "body", tree: twice }});
const first = body.children[0].children[1];
first.focus(); first.selectionStart = 2; first.selectionEnd = 3;
dom({{ _class: "Render", into: "body", tree: twice }});
const second = body.children[0].children[1];
check("the caret did not survive an incremental render", [second === first, second.focused, second.selectionStart, second.selectionEnd], [true, true, 2, 3]);
dom({{ _class: "Render", into: "body", tree: {{ tag: "div", children: [ {{ tag: "p" }}, {{ tag: "button" }} ] }} }});
check("a different node at the caret's place was focused", body.children[0].children[1].focused, undefined);

// A box someone typed in keeps their words across a render that says the
// same thing — and lets go of them when the program draws a different
// value. The attribute is the default; the live value is what is shown, and
// a kept element has to be told about a change to it.
const search = (v) => ({{ tag: "div", children: [ {{ tag: "input", attrs: {{ type: "search", value: v }} }} ] }});
dom({{ _class: "Render", into: "body", tree: search("") }});
const box2 = body.children[0].children[0];
box2.value = "task";
dom({{ _class: "Render", into: "body", tree: search("") }});
check("an unchanged value attribute wiped what was typed", [body.children[0].children[0] === box2, box2.value], [true, "task"]);
dom({{ _class: "Render", into: "body", tree: search("") }});
box2.setAttribute("value", "task");
dom({{ _class: "Render", into: "body", tree: search("done") }});
check("a changed value attribute did not reach the live value", box2.value, "done");
dom({{ _class: "Render", into: "body", tree: search("") }});
check("clearing the value attribute did not clear the box", box2.value, "");

const download = dom({{ _class: "Download", uri: "data:text/plain;base64,aGk=", name: "hello.txt" }});
check("a download did not answer success", download, {{ _class: "DownloadResult", ok: true }});
check("the download link was not clicked with its URI and name", [downloaded.attrs.href, downloaded.attrs.download, downloaded.attrs.rel], ["data:text/plain;base64,aGk=", "hello.txt", "noopener"]);
check("the temporary download link was left on the page", body.children.includes(downloaded), false);

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

/// A video too large to carry whole is held under a `handle` and read in
/// parts, each with its sha256; and what the video says about itself —
/// Apple's creation date and offset, its length, codec and camera — comes
/// with it, read from the `moov` box alone. Node's own `File` stands in for
/// the browser's.
#[test]
fn a_large_video_is_held_read_in_parts_and_says_what_it_is() {
    if !tool_exists("node") {
        eprintln!("skipped: needs node");
        return;
    }
    let half = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/dom/page.mjs");
    let dir = std::env::temp_dir().join(format!("code-dom-large-file-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create the probe dir");
    let probe = dir.join("probe.mjs");
    let script = r#"import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
const half = new Function("return (" + readFileSync(HALF, "utf8") + ")")();
globalThis.FileReader = class {
  readAsArrayBuffer(b) { b.arrayBuffer().then((r) => { this.result = r; this.onload(); }, () => this.onerror()); }
  readAsDataURL(b) { b.arrayBuffer().then((r) => { this.result = "data:;base64," + Buffer.from(r).toString("base64"); this.onload(); }); }
};
const check = (what, got, want) => {
  if (JSON.stringify(got) !== JSON.stringify(want)) throw new Error(`${what}: got ${JSON.stringify(got)}, wanted ${JSON.stringify(want)}`);
};

// A QuickTime file's boxes, built by hand: size, type, body.
const u32 = (n) => { const b = Buffer.alloc(4); b.writeUInt32BE(n); return b; };
const box = (type, ...parts) => { const body = Buffer.concat(parts); return Buffer.concat([u32(body.length + 8), Buffer.from(type, "latin1"), body]); };
const mvhd = box("mvhd", u32(0), u32(0), u32(0), u32(600), u32(7584), Buffer.alloc(80));
const hdlr = box("hdlr", u32(0), u32(0), Buffer.from("vide"), Buffer.alloc(13));
const stsd = box("stsd", u32(0), u32(1), u32(16), Buffer.from("hvc1"), Buffer.alloc(8));
const trak = box("trak", box("mdia", hdlr, box("minf", box("stbl", stsd))));
const names = ["com.apple.quicktime.creationdate", "com.apple.quicktime.make", "com.apple.quicktime.model"];
const values = ["2026-09-27T09:46:06+0200", "Apple", "iPhone 11"];
const keys = box("keys", u32(0), u32(names.length), ...names.map((n) => Buffer.concat([u32(n.length + 8), Buffer.from("mdta"), Buffer.from(n)])));
const ilst = box("ilst", ...values.map((v, i) => Buffer.concat([u32(8 + 16 + v.length), u32(i + 1), box("data", u32(1), u32(0), Buffer.from(v))])));
const moov = box("moov", mvhd, trak, box("meta", keys, ilst));
const head = box("ftyp", Buffer.from("qt  "), u32(0));
const film = Buffer.concat([head, box("mdat", Buffer.alloc(13 * 1024 * 1024, 7)), moov]);

const fired = [];
class El {
  constructor(t) { this.tagName = t.toUpperCase(); this.nodeType = 1; this.children = []; this.attrs = {}; this.listeners = {}; this.files = []; }
  addEventListener(n, f) { (this.listeners[n] ||= []).push(f); }
  setAttribute(k, v) { this.attrs[k] = String(v); }
  hasAttribute(k) { return k in this.attrs; }
  getAttribute(k) { return this.attrs[k]; }
  appendChild(c) { c.parentNode = this; this.children.push(c); return c; }
  replaceChildren(c) { this.children = []; this.appendChild(c); }
  querySelector() { return null; }
  send(n, e) { for (const f of this.listeners[n] || []) f({ target: this, ...e }); }
}
const body = new El("body");
const doc = { createElement: (t) => new El(t), createTextNode: (s) => ({ nodeType: 3, text: s }), body, querySelector: (s) => (s === "body" ? body : null), getElementById: () => null, head: body, activeElement: null };
const [, dom] = half({ doc, fire: (p) => fired.push(p) });
dom({ _class: "Render", into: "body", tree: { tag: "input", attrs: { type: "file" }, on: { change: "Chosen" } } });
const settle = () => new Promise((r) => setTimeout(r, 200));

body.children[0].files = [new File([film], "IMG_2681.MOV", { type: "video/quicktime", lastModified: 1790000000000 })];
body.children[0].send("change", {});
await settle();
check("a large video was not held and described", fired, [{ _class: "Chosen", value: "IMG_2681.MOV", file: {
  name: "IMG_2681.MOV", type: "video/quicktime", size: film.length, last_modified: 1790000000,
  video: { seconds: 12.64, codec: "hvc1", taken_local: "2026-09-27 09:46:06", offset: 120, make: "Apple", model: "iPhone 11" },
  data_base64: "", refused: "larger than 12 MB", handle: "file-1" } }]);
fired.length = 0;

// A part: answered at once, the bytes later as the application's particle.
const asked = dom({ _class: "ReadFilePart", handle: "file-1", start: 8, length: 4, then: { _class: "Part", upload: "u1" } });
check("ReadFilePart did not answer at once", asked, { _class: "ReadFilePartResult", ok: true });
await settle();
const four = film.subarray(8, 12);
check("the part did not come back", fired, [{ _class: "Part", upload: "u1", handle: "file-1", start: 8, size: film.length,
  length: 4, data_base64: four.toString("base64"), sha256: createHash("sha256").update(four).digest("hex") }]);
fired.length = 0;

// The last part is what is left; an unknown handle is told no; a forgotten
// file is gone.
dom({ _class: "ReadFilePart", handle: "file-1", start: film.length - 3, length: 1000, then: "Part" });
await settle();
check("the last part was not what was left", [fired[0].length, fired[0].data_base64], [3, film.subarray(film.length - 3).toString("base64")]);
check("an unknown handle was read", dom({ _class: "ReadFilePart", handle: "file-9", start: 0, length: 1, then: "Part" }), { _class: "ReadFilePartResult", ok: false });
check("forgetting a held file", dom({ _class: "ForgetFile", handle: "file-1" }), { _class: "ForgetFileResult", existed: true });
check("a forgotten file was read", dom({ _class: "ReadFilePart", handle: "file-1", start: 0, length: 1, then: "Part" }), { _class: "ReadFilePartResult", ok: false });
"#
    .replace("HALF", &format!("{half:?}"));
    fs::write(&probe, script).expect("write the probe");
    let output = Command::new("node")
        .arg(&probe)
        .output()
        .expect("run the probe under node");
    assert!(
        output.status.success(),
        "dom did not hold a large video: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

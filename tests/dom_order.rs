//! A render that drops, swaps or adds a child does not move the kept
//! children after it. A browser takes focus from a node that is moved, so an
//! open dialog after a notice that went used to lose its caret mid-typing.

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
fn a_child_going_does_not_move_the_ones_after_it() {
    if !tool_exists("node") {
        eprintln!("skipped: needs node");
        return;
    }
    let half = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/dom/page.mjs");
    let dir = std::env::temp_dir().join(format!("code-dom-order-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create the probe dir");
    let probe = dir.join("probe.mjs");
    let source = fs::read_to_string(&half).expect("read the page half");
    let mut script = format!("const half = new Function(\"return (\" + {source:?} + \")\")();\n");
    script.push_str(
        r##"
let doc;
let moves = 0;

// Just enough of a DOM, with the one browser rule this is about: moving a
// node that holds focus takes the focus away.
class El {
  constructor(tag) {
    this.tagName = tag.toUpperCase(); this.nodeType = 1; this.childNodes = [];
    this.parentNode = null; this.attrs = {}; this.listeners = {};
  }
  get children() { return this.childNodes; }
  get isConnected() { for (let n = this; n; n = n.parentNode) if (n === doc.body) return true; return false; }
  addEventListener(name, fn) { (this.listeners[name] ||= []).push(fn); }
  removeEventListener() {}
  setAttribute(k, v) { this.attrs[k] = String(v); if (k === "id") this.id = String(v); }
  removeAttribute(k) { delete this.attrs[k]; }
  hasAttribute(k) { return k in this.attrs; }
  getAttribute(k) { return this.attrs[k] ?? null; }
  detach(child) {
    if (child.parentNode) {
      if (child.contains && child.contains(doc.activeElement)) doc.activeElement = doc.body;
      child.parentNode.childNodes = child.parentNode.childNodes.filter(c => c !== child);
    }
  }
  appendChild(child) { return this.insertBefore(child, null); }
  insertBefore(child, before) {
    if (child.parentNode) moves += 1;
    this.detach(child);
    const at = before ? this.childNodes.indexOf(before) : -1;
    if (at < 0) this.childNodes.push(child); else this.childNodes.splice(at, 0, child);
    child.parentNode = this;
    return child;
  }
  removeChild(child) { this.detach(child); child.parentNode = null; return child; }
  replaceChildren(...children) {
    for (const old of [...this.childNodes]) this.removeChild(old);
    for (const child of children) this.appendChild(child);
  }
  focus() { doc.activeElement = this; }
  contains(other) { for (let n = other; n; n = n.parentNode) if (n === this) return true; return false; }
  walk() { return this.childNodes.flatMap(c => [c, ...(c.walk ? c.walk() : [])]); }
  querySelector(sel) {
    if (sel === "[autofocus]") return this.walk().find(c => c.nodeType === 1 && c.hasAttribute("autofocus")) || null;
    return null;
  }
  querySelectorAll() { return []; }
}

const body = new El("body");
const app = new El("main"); app.setAttribute("id", "app");
doc = {
  body, activeElement: null, listeners: {},
  scrollingElement: body,
  createElement: tag => new El(tag),
  createTextNode: text => ({ nodeType: 3, nodeValue: String(text), childNodes: [], parentNode: null }),
  querySelector: sel => sel === "#app" ? app : (sel === "body" ? body : body.querySelector(sel)),
  querySelectorAll: () => [],
  addEventListener: (name, fn) => (doc.listeners[name] ||= []).push(fn),
  getElementById: id => body.walk().find(c => c.id === id) || null,
  head: body,
};
body.appendChild(app);
doc.activeElement = body;
const [, dom] = half({ doc, fire: () => {} });
const check = (what, got, want) => {
  if (got !== want) throw new Error(`${what}: got ${got}, wanted ${want}`);
};

const part = (key, tag = "div", extra = {}) => ({ tag, attrs: { class: key, "data-code-key": key }, ...extra });
const dialog = { tag: "dialog", attrs: { class: "editor", "data-code-key": "dialog", open: "open" }, children: [{ tag: "textarea", attrs: { class: "subject" } }] };
const page = (...children) => ({ tag: "div", attrs: { class: "page" }, children });
const render = (tree) => dom({ _class: "Render", into: "#app", tree });

render(page(part("session"), part("notice", "p"), part("toolbar"), dialog));
const root = app.childNodes[0];
const box = root.walk().find(c => c.tagName === "TEXTAREA");
box.focus();
const shown = () => root.childNodes.map(c => c.getAttribute("class")).join(",");

// The notice goes: nothing after it moves, the caret stays.
moves = 0;
render(page(part("session"), part("toolbar"), dialog));
check("the parts after the notice", shown(), "session,toolbar,editor");
check("kept parts moved when the notice went", moves, 0);
check("the caret left the dialog when the notice went", doc.activeElement === box, true);

// A part before the dialog is drawn anew (its events changed): the new one
// takes the old one's place, the dialog stays put.
moves = 0;
render(page(part("session"), part("toolbar", "div", { on: { click: "Tapped" } }), dialog));
check("the parts after a swapped toolbar", shown(), "session,toolbar,editor");
check("kept parts moved when the toolbar was drawn anew", moves, 0);
check("the caret left the dialog when the toolbar was drawn anew", doc.activeElement === box, true);

// A part comes back: still nothing moves.
moves = 0;
render(page(part("session"), part("notice", "p"), part("toolbar", "div", { on: { click: "Tapped" } }), dialog));
check("the parts after the notice came back", shown(), "session,notice,toolbar,editor");
check("kept parts moved when the notice came back", moves, 0);
check("the caret left the dialog when the notice came back", doc.activeElement === box, true);
check("the dialog was drawn anew", root.walk().find(c => c.tagName === "TEXTAREA") === box, true);

// Kept parts asked for in another order are still put in that order.
render(page(dialog, part("toolbar", "div", { on: { click: "Tapped" } }), part("notice", "p"), part("session")));
check("a new order", shown(), "editor,toolbar,notice,session");
"##,
    );
    fs::write(&probe, script).expect("write the probe");

    let output = Command::new("node")
        .arg(&probe)
        .output()
        .expect("run the probe under node");
    assert!(
        output.status.success(),
        "dom moved kept children: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

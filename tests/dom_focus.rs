//! The page half keeps keyboard focus inside an open modal and returns it to
//! the control that opened the modal after the next render removes it.

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
fn a_modal_traps_tab_and_restores_its_opener() {
    if !tool_exists("node") {
        eprintln!("skipped: needs node");
        return;
    }
    let half = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/dom/page.mjs");
    let dir = std::env::temp_dir().join(format!("code-dom-focus-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create the probe dir");
    let probe = dir.join("probe.mjs");
    let source = fs::read_to_string(&half).expect("read the page half");
    let mut script = format!(
        "const half = new Function(\"return (\" + {} + \")\")();\n",
        format!("{source:?}")
    );
    script.push_str(
        r##"
let doc;

class El {
  constructor(tag) {
    this.tagName = tag.toUpperCase(); this.nodeType = 1; this.children = [];
    this.parentNode = null; this.attrs = {}; this.listeners = {};
    this.isConnected = false; this.focused = false;
  }
  addEventListener(name, fn) { (this.listeners[name] ||= []).push(fn); }
  setAttribute(k, v) { this.attrs[k] = String(v); if (k === "id") this.id = String(v); }
  hasAttribute(k) { return k in this.attrs; }
  getAttribute(k) { return this.attrs[k] ?? null; }
  appendChild(child) {
    child.parentNode = this; this.children.push(child); child.isConnected = this.isConnected;
    for (const descendant of child.children || []) descendant.isConnected = child.isConnected;
    return child;
  }
  removeChild(child) {
    this.children = this.children.filter(c => c !== child); child.parentNode = null; child.isConnected = false; return child;
  }
  replaceChildren(...children) {
    for (const old of this.children) { old.parentNode = null; old.isConnected = false; }
    this.children = []; for (const child of children) this.appendChild(child);
  }
  focus() { this.focused = true; doc.activeElement = this; }
  contains(other) { for (let n = other; n; n = n.parentNode) if (n === this) return true; return false; }
  walk() { return this.children.flatMap(c => [c, ...(c.walk ? c.walk() : [])]); }
  querySelector(sel) {
    if (sel === "[autofocus]") return this.walk().find(c => c.nodeType === 1 && c.hasAttribute("autofocus")) || null;
    if (sel === ".fab") return this.walk().find(c => c.nodeType === 1 && String(c.getAttribute("class") || "").split(/\s+/).includes("fab")) || null;
    if (sel === 'dialog[aria-modal="true"][open]') return this.walk().find(c => c.tagName === "DIALOG" && c.getAttribute("aria-modal") === "true" && c.hasAttribute("open")) || null;
    return null;
  }
  querySelectorAll(sel) {
    const all = this.walk().filter(c => c.nodeType === 1);
    if (sel === 'button, input, textarea, select, a[href], [tabindex]:not([tabindex="-1"])') {
      return all.filter(c => ["BUTTON", "INPUT", "TEXTAREA", "SELECT"].includes(c.tagName) || (c.tagName === "A" && c.hasAttribute("href")) || (c.hasAttribute("tabindex") && c.getAttribute("tabindex") !== "-1"));
    }
    if (sel === 'dialog[aria-modal="true"][open]') return all.filter(c => c.tagName === "DIALOG" && c.getAttribute("aria-modal") === "true" && c.hasAttribute("open"));
    return [];
  }
}

const body = new El("body"); body.isConnected = true;
const app = new El("main"); app.setAttribute("id", "app"); body.appendChild(app);
const opener = new El("button"); opener.setAttribute("class", "fab"); app.appendChild(opener);
doc = {
  body, activeElement: opener, listeners: {},
  createElement: tag => new El(tag),
  createTextNode: text => ({ nodeType: 3, text: String(text), children: [] }),
  querySelector: sel => sel === "#app" ? app : (sel === "body" ? body : body.querySelector(sel)),
  querySelectorAll: sel => body.querySelectorAll(sel),
  addEventListener: (name, fn) => (doc.listeners[name] ||= []).push(fn),
  getElementById: id => body.walk().find(c => c.id === id) || null,
  head: body,
};
const [, dom] = half({ doc, fire: () => {} });
const check = (what, got, want) => {
  if (got !== want) throw new Error(`${what}: got ${got}, wanted ${want}`);
};
const sendKey = (key, shiftKey) => {
  let prevented = false;
  const event = { key, shiftKey, preventDefault: () => { prevented = true; } };
  for (const fn of doc.listeners.keydown || []) fn(event);
  return prevented;
};

const openTree = { tag: "div", children: [
  { tag: "button", attrs: { class: "fab" } },
  { tag: "dialog", attrs: { open: "open", "aria-modal": "true", tabindex: "-1" }, children: [
    { tag: "input", attrs: { autofocus: "" } },
    { tag: "button" },
    { tag: "button" },
  ] },
] };
dom({ _class: "Render", into: "#app", tree: openTree });
const dialog = app.querySelector('dialog[aria-modal="true"][open]');
const controls = dialog.querySelectorAll('button, input, textarea, select, a[href], [tabindex]:not([tabindex="-1"])');
check("opening a modal did not focus its autofocus control", doc.activeElement === controls[0], true);
controls[controls.length - 1].focus();
check("Tab at the end did not wrap", sendKey("Tab", false), true);
check("Tab escaped the modal", doc.activeElement === controls[0], true);
controls[0].focus();
check("Shift+Tab at the beginning did not wrap", sendKey("Tab", true), true);
check("Shift+Tab escaped the modal", doc.activeElement === controls[controls.length - 1], true);

dom({ _class: "Render", into: "#app", tree: { tag: "div", children: [{ tag: "button", attrs: { class: "fab" } }] } });
const replacement = app.querySelector(".fab");
check("closing a modal did not restore its opener", doc.activeElement === replacement, true);
"##,
    );
    fs::write(&probe, script).expect("write the probe");

    let output = Command::new("node")
        .arg(&probe)
        .output()
        .expect("run the probe under node");
    assert!(
        output.status.success(),
        "dom's modal focus scope did not hold: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

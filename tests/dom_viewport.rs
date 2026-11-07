//! `dom`'s page half: a map or a timeline carried, pinched and wheeled
//! (`viewport`), and a photo chosen in a file box saying when and where it
//! was taken (`file.photo`) and when the file was last changed
//! (`file.last_modified`). Driven under node against the half itself with
//! stand-in elements, files and readers.

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
fn a_viewport_is_carried_pinched_and_wheeled_and_a_photo_says_where() {
    if !tool_exists("node") {
        eprintln!("skipped: needs node");
        return;
    }
    let half = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/dom/page.mjs");
    let dir = std::env::temp_dir().join(format!("code-dom-viewport-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create the probe dir");
    let probe = dir.join("probe.mjs");
    fs::write(
        &probe,
        format!(
            r#"import {{ readFileSync }} from "node:fs";
const half = new Function("return (" + readFileSync({half:?}, "utf8") + ")")();

// An element that remembers its listeners and where the test says it is.
class El {{
  constructor(tag) {{
    this.tagName = tag.toUpperCase(); this.nodeType = 1; this.children = []; this.parentNode = null;
    this.attrs = {{}}; this.listeners = {{}}; this.rect = {{ top: 0, height: 0 }}; this.captured = null; this.namespaceURI = null;
    this.scrollTop = 0; this.scrollHeight = 0; this.clientHeight = 0;
    this.style = {{ transition: "", transform: "", willChange: "" }};
  }}
  addEventListener(name, fn) {{ (this.listeners[name] ||= []).push(fn); }}
  setAttribute(k, v) {{ this.attrs[k] = String(v); }}
  removeAttribute(k) {{ delete this.attrs[k]; }}
  hasAttribute(k) {{ return k in this.attrs; }}
  getAttribute(k) {{ return this.attrs[k]; }}
  appendChild(c) {{ c.parentNode = this; this.children.push(c); return c; }}
  insertBefore(c, before) {{
    if (c.parentNode === this) this.children = this.children.filter(child => child !== c);
    else if (c.parentNode && c.parentNode.removeChild) c.parentNode.removeChild(c);
    const at = before ? this.children.indexOf(before) : -1;
    c.parentNode = this;
    if (at < 0) this.children.push(c); else this.children.splice(at, 0, c);
    return c;
  }}
  removeChild(c) {{ this.children = this.children.filter(child => child !== c); c.parentNode = null; return c; }}
  replaceChildren(c) {{ for (const old of this.children) old.parentNode = null; this.children = []; this.appendChild(c); }}
  getBoundingClientRect() {{ return this.rect; }}
  setPointerCapture(id) {{ this.captured = id; }}
  // Straight to this element's listeners: bubbling is the browser's, and
  // the half listens on the node the tree named.
  send(name, e) {{
    const event = {{ target: this, pointerId: 1, timeStamp: 0, clientX: 0, clientY: 0, ...e,
      defaultPrevented: false, immediateStopped: false,
      preventDefault() {{ this.defaultPrevented = true; }},
      stopImmediatePropagation() {{ this.immediateStopped = true; }}
    }};
    for (const fn of this.listeners[name] || []) {{ fn(event); if (event.immediateStopped) break; }}
    return event;
  }}
}}
const body = new El("body");
const doc = {{
  createElement: (t) => new El(t),
  createElementNS: (ns, t) => {{ const el = new El(t); el.namespaceURI = ns; return el; }},
  createTextNode: (s) => ({{ nodeType: 3, text: String(s) }}),
  querySelector: (sel) => (sel === "body" ? body : null),
  getElementById: () => null,
  head: body,
}};

const fired = [];
const [, dom] = half({{ doc, fire: (p) => fired.push(p) }});
const check = (what, got, want) => {{
  if (JSON.stringify(got) !== JSON.stringify(want)) throw new Error(`${{what}}: got ${{JSON.stringify(got)}}, wanted ${{JSON.stringify(want)}}`);
}};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

dom({{ _class: "Render", into: "body", tree: {{ tag: "div", children: [
  {{ tag: "div", on: {{ viewport: {{ _class: "Moved", view: "map" }} }}, children: [
    {{ tag: "div", attrs: {{ class: "tiles" }}, children: [{{ tag: "button", on: {{ click: "Marker" }} }}] }},
  ] }},
  {{ tag: "input", attrs: {{ type: "file" }}, on: {{ change: "Chosen" }} }},
] }} }});
const map = body.children[0].children[0];
const surface = map.children[0];
const marker = surface.children[0];
map.rect = {{ left: 10, top: 20, width: 300, height: 200 }};

// One finger carries it: the surface follows, one particle at the end.
map.send("pointerdown", {{ pointerId: 1, clientX: 110, clientY: 120 }});
map.send("pointermove", {{ pointerId: 1, clientX: 140, clientY: 100 }});
check("the surface did not follow the finger", surface.style.transform, "translate(30px, -20px) scale(1)");
check("a carry sent before it was let go", fired, []);
map.send("pointerup", {{ pointerId: 1, clientX: 150, clientY: 90 }});
check("a carry", fired, [{{ _class: "Moved", view: "map", dx: 40, dy: -30, scale: 1, x: 100, y: 100, width: 300, height: 200 }}]);
fired.length = 0;
const swallowed = map.send("click", {{}});
check("the click after a carry was not swallowed", swallowed.immediateStopped, true);

// A press that hardly moves is a tap: the marker's click still lands.
map.send("pointerdown", {{ pointerId: 1, clientX: 50, clientY: 50 }});
map.send("pointermove", {{ pointerId: 1, clientX: 51, clientY: 51 }});
map.send("pointerup", {{ pointerId: 1, clientX: 51, clientY: 51 }});
check("a tap was taken for a carry", fired, []);
const tap = map.send("click", {{}});
check("a tap's click was swallowed", tap.immediateStopped, false);

// Two fingers pinch it about their middle.
map.send("pointerdown", {{ pointerId: 1, clientX: 110, clientY: 120 }});
map.send("pointerdown", {{ pointerId: 2, clientX: 210, clientY: 120 }});
map.send("pointermove", {{ pointerId: 1, clientX: 60, clientY: 120 }});
map.send("pointermove", {{ pointerId: 2, clientX: 260, clientY: 120 }});
map.send("pointerup", {{ pointerId: 2, clientX: 260, clientY: 120 }});
check("a pinch sent before both were let go", fired, []);
map.send("pointerup", {{ pointerId: 1, clientX: 60, clientY: 120 }});
check("a pinch", fired, [{{ _class: "Moved", view: "map", dx: 0, dy: 0, scale: 2, x: 150, y: 100, width: 300, height: 200 }}]);
fired.length = 0;

// A wheel zooms about the cursor, once it has settled.
map.send("wheel", {{ clientX: 60, clientY: 70, deltaY: -100 }});
map.send("wheel", {{ clientX: 60, clientY: 70, deltaY: -100 }});
check("a wheel sent before it settled", fired, []);
await sleep(260);
check("a wheel", fired.map((p) => [p.x, p.y, p.dx, p.scale]), [[50, 50, 0, Math.round(Math.exp(0.4) * 1000) / 1000]]);
fired.length = 0;
await sleep(300);
check("the surface kept its own transform", surface.style.transform, "");

// A photo says when and where it was taken; any file says when it changed.
const bytes = Buffer.from("/9j/4QDNRXhpZgAATU0AKgAAAAgAAodpAAQAAAABAAAAJoglAAQAAAABAAAAXwAAAAAAApADAAIAAAAUAAAARJARAAIAAAAHAAAAWAAAAAAyMDI2OjEyOjAyIDE5OjA1OjMzACswMzowMAAABAABAAIAAAACTgAAAAACAAUAAAADAAAAlQADAAIAAAACRQAAAAAEAAUAAAADAAAArQAAAAAAABAEAAAAZAAAAAAAAABkAAALuAAAAGQAAArwAAAAZAAAFqgAAABkAAASwAAAAGT/2Q==", "base64");
const box = body.children[0].children[1];
class Blob {{ constructor(buf, type) {{ this.buf = buf; this.type = type; this.size = buf.length; }} slice(a, b) {{ return new Blob(this.buf.subarray(a, b), this.type); }} }}
globalThis.FileReader = class {{
  readAsDataURL(b) {{ setTimeout(() => {{ this.result = "data:" + b.type + ";base64," + b.buf.toString("base64"); this.onload(); }}, 0); }}
  readAsArrayBuffer(b) {{ setTimeout(() => {{ this.result = b.buf.buffer.slice(b.buf.byteOffset, b.buf.byteOffset + b.buf.byteLength); this.onload(); }}, 0); }}
}};
const photo = Object.assign(new Blob(bytes, "image/jpeg"), {{ name: "istanbul.jpg", lastModified: 1796000000999 }});
box.files = [photo];
box.send("change", {{}});
await sleep(20);
check("the photo's facts", fired[0].file.photo, {{ taken_local: "2026-12-02 19:05:33", offset: 180, lat: 41.008333, lon: 28.98 }});
check("the file's date", fired[0].file.last_modified, 1796000000);
check("the bytes", fired[0].file.data_base64, "/9j/4QDNRXhpZgAATU0AKgAAAAgAAodpAAQAAAABAAAAJoglAAQAAAABAAAAXwAAAAAAApADAAIAAAAUAAAARJARAAIAAAAHAAAAWAAAAAAyMDI2OjEyOjAyIDE5OjA1OjMzACswMzowMAAABAABAAIAAAACTgAAAAACAAUAAAADAAAAlQADAAIAAAACRQAAAAAEAAUAAAADAAAArQAAAAAAABAEAAAAZAAAAAAAAABkAAALuAAAAGQAAArwAAAAZAAAFqgAAABkAAASwAAAAGT/2Q==");
fired.length = 0;
const voice = Object.assign(new Blob(Buffer.from("OggS"), "audio/ogg"), {{ name: "voice.ogg", lastModified: 1796000500000 }});
box.files = [voice];
box.send("change", {{}});
await sleep(20);
check("a recording says no photo facts", "photo" in fired[0].file, false);
check("a recording's file date", fired[0].file.last_modified, 1796000500);
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
        "dom's viewport or photo facts did not hold: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

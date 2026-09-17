//! `dom`'s page half reads gestures — a swipe, a double tap, a child carried
//! among its siblings — and sends the particle the tree named for them, once.
//!
//! Driven under node against the half itself with a stand-in element, since
//! what is under test is the reading of pointer events and nothing else: no
//! browser, no wasm, no layout beyond the rectangles the test writes.

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
fn a_page_reads_gestures_and_sends_the_particle_once() {
    if !tool_exists("node") {
        eprintln!("skipped: needs node");
        return;
    }
    let half = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/dom/page.mjs");
    let dir = std::env::temp_dir().join(format!("code-dom-gestures-{}", std::process::id()));
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

const r = dom({{ _class: "Render", into: "body", tree: {{
  tag: "ul", on: {{ reorder: "Move" }}, children: [
    {{ tag: "li", attrs: {{ value: "a", "data-focus-key": "a" }}, on: {{ swipeleft: {{ _class: "Gone", id: 1 }}, click: "Tapped", doubletap: "Cycle" }}, children: [{{ tag: "svg", children: [{{ tag: "path" }}] }}] }},
    {{ tag: "li", attrs: {{ value: "b", "data-focus-key": "b" }} }},
    {{ tag: "li", attrs: {{ value: "c", "data-focus-key": "c" }}, on: {{ swiperight: "Back" }} }},
    {{ tag: "div", on: {{ edgeclose: "Close" }}, children: [
      {{ tag: "div", attrs: {{ class: "backdrop" }} }},
      {{ tag: "div", attrs: {{ class: "editor" }} }},
    ] }},
    {{ tag: "div", on: {{ edgeclose: "Close" }}, children: [
      {{ tag: "div", attrs: {{ class: "backdrop" }} }},
      {{ tag: "div", attrs: {{ class: "editor" }} }},
    ] }},
    {{ tag: "div", on: {{ edgeclose: "Close" }}, children: [
      {{ tag: "div", attrs: {{ class: "backdrop" }} }},
      {{ tag: "div", attrs: {{ class: "editor" }} }},
    ] }},
  ] }} }});
check("render refused", r, {{ _class: "RenderResult", ok: true }});
const list = body.children[0];
const [a, b, c] = list.children;
const edge = list.children[3];
const edgeBottom = list.children[4];
const edgeWheel = list.children[5];
const edgeSurface = edge.children[1];
const edgeBottomSurface = edgeBottom.children[1];
[a, b, c].forEach((li, i) => {{ li.rect = {{ top: i * 40, height: 40 }}; }});
edge.rect = {{ top: 120, height: 40 }};
edgeBottom.rect = {{ top: 160, height: 40 }};
edgeWheel.rect = {{ top: 200, height: 40 }};
check("the svg was created in the svg namespace", a.children[0].namespaceURI, "http://www.w3.org/2000/svg");
check("the svg path was created in the svg namespace", a.children[0].children[0].namespaceURI, "http://www.w3.org/2000/svg");

// A swipe left: far enough, fast enough, along the one axis — with what the
// element holds, as any event carries.
a.send("pointerdown", {{ clientX: 200, clientY: 20, timeStamp: 1000 }});
a.send("pointerup", {{ clientX: 100, clientY: 25, timeStamp: 1300 }});
check("a swipe left did not send its particle", fired, [{{ _class: "Gone", id: 1, value: "a" }}]);
fired.length = 0;
const blockedClick = a.send("click", {{}});
check("a click synthesized after a swipe", fired, []);
check("the synthesized click was not prevented", blockedClick.defaultPrevented, true);
fired.length = 0;
// A real press on the now-visible confirmation control must still work if a
// browser did not synthesize a click for the swipe.
a.send("pointerdown", {{ clientX: 50, clientY: 20, timeStamp: 2000 }});
a.send("pointerup", {{ clientX: 50, clientY: 20, timeStamp: 2050 }});
a.send("click", {{}});
check("a later deliberate click was swallowed", fired, [{{ _class: "Tapped", value: "a" }}]);
fired.length = 0;

// Too slow, too short, too diagonal, the wrong way: nothing.
a.send("pointerdown", {{ clientX: 200, clientY: 20, timeStamp: 1000 }});
a.send("pointerup", {{ clientX: 100, clientY: 20, timeStamp: 2500 }});
a.send("pointerdown", {{ clientX: 200, clientY: 20, timeStamp: 1000 }});
a.send("pointerup", {{ clientX: 180, clientY: 20, timeStamp: 1100 }});
a.send("pointerdown", {{ clientX: 200, clientY: 20, timeStamp: 1000 }});
a.send("pointerup", {{ clientX: 100, clientY: 120, timeStamp: 1100 }});
a.send("pointerdown", {{ clientX: 100, clientY: 20, timeStamp: 1000 }});
a.send("pointerup", {{ clientX: 200, clientY: 20, timeStamp: 1100 }});
check("a swipe that was not one still sent something", fired, []);

// The other way, on the row that asked for it.
c.send("pointerdown", {{ clientX: 100, clientY: 100, timeStamp: 1000 }});
c.send("pointerup", {{ clientX: 200, clientY: 100, timeStamp: 1100 }});
check("a swipe right did not send its particle", fired, [{{ _class: "Back", value: "c" }}]);
fired.length = 0;

// A pull follows the finger. A short pull settles back and does not close.
edge.clientHeight = 100; edge.scrollHeight = 300; edge.scrollTop = 0;
edge.send("touchstart", {{ touches: [{{ clientY: 20 }}] }});
const shortPull = edge.send("touchmove", {{ touches: [{{ clientY: 65 }}] }});
check("a short outward pull closed too soon", fired, []);
check("the short pull was allowed to bounce", shortPull.defaultPrevented, true);
check("the surface followed a short pull", edgeSurface.style.transform, "translate3d(0, 45px, 0)");
check("the backdrop stayed fixed during a short pull", edge.children[0].style.transform, "");
edge.send("touchend", {{}});
await new Promise(resolve => setTimeout(resolve, 230));
check("a short pull did not settle back", fired, []);
check("the short pull did not keep its transform", edgeSurface.style.transform, "");

// A pull that starts in the middle remains ordinary page scrolling.
edge.scrollTop = 100;
edge.send("touchstart", {{ touches: [{{ clientY: 20 }}] }});
edge.send("touchmove", {{ touches: [{{ clientY: 80 }}] }});
check("a pull from the middle closed the dialog", fired, []);
edge.send("touchend", {{}});
// The bottom edge has the same follow-and-settle behavior in the opposite
// direction, and crosses the threshold only when the finger is released.
edgeBottom.clientHeight = 100; edgeBottom.scrollHeight = 300; edgeBottom.scrollTop = 200;
edgeBottom.send("pointerdown", {{ pointerType: "touch", clientY: 100 }});
const bottomPull = edgeBottom.send("pointermove", {{ pointerType: "touch", clientY: 25 }});
check("a bottom pull closed too soon", fired, []);
check("the bottom pull was allowed to bounce", bottomPull.defaultPrevented, true);
check("the surface followed the bottom pull", edgeBottomSurface.style.transform, "translate3d(0, -75px, 0)");
edgeBottom.send("pointerup", {{ pointerType: "touch", clientY: 25 }});
await new Promise(resolve => setTimeout(resolve, 230));
check("a short bottom pull did not settle back", edgeBottomSurface.style.transform, "");
edgeBottom.send("pointerdown", {{ pointerType: "touch", clientY: 100 }});
edgeBottom.send("pointermove", {{ pointerType: "touch", clientY: -10 }});
edgeBottom.send("pointerup", {{ pointerType: "touch", clientY: -10 }});
check("the bottom close fired before its animation", fired, []);
check("the bottom close started its slide-away animation", edgeBottomSurface.style.transform, "translate3d(0, -100%, 0)");
await new Promise(resolve => setTimeout(resolve, 230));
check("the bottom close did not send its particle", fired, [{{ _class: "Close" }}]);
fired.length = 0;

// A long top pull closes only after release, after its slide-away animation.
edge.scrollTop = 0;
edge.send("pointerdown", {{ pointerType: "touch", clientY: 20 }});
edge.send("touchstart", {{ touches: [{{ clientY: 20 }}] }});
const topPull = edge.send("touchmove", {{ touches: [{{ clientY: 125 }}] }});
check("a long outward pull closed before release", fired, []);
check("the long top pull was allowed to bounce", topPull.defaultPrevented, true);
edge.send("touchend", {{ changedTouches: [{{ clientY: 125 }}] }});
check("the top close fired before its animation", fired, []);
check("the top close started its slide-away animation", edgeSurface.style.transform, "translate3d(0, 100%, 0)");
await new Promise(resolve => setTimeout(resolve, 230));
check("the top close did not send its particle", fired, [{{ _class: "Close" }}]);
fired.length = 0;

// A wheel event still closes at a boundary, after the same animation.
edgeWheel.clientHeight = 100; edgeWheel.scrollHeight = 300; edgeWheel.scrollTop = 0;
const wheel = edgeWheel.send("wheel", {{ deltaY: -20 }});
check("an outward wheel closed before its animation", fired, []);
check("the outward wheel was allowed to overscroll", wheel.defaultPrevented, true);
check("the wheel close left the backdrop fixed", edgeWheel.children[0].style.transform, "");
await new Promise(resolve => setTimeout(resolve, 230));
check("the outward wheel did not send its particle", fired, [{{ _class: "Close" }}]);
fired.length = 0;

// Two taps close together are one double; a third alone is nothing yet.
a.send("pointerdown", {{ clientX: 50, clientY: 20, timeStamp: 5000 }});
a.send("pointerup", {{ clientX: 50, clientY: 20, timeStamp: 5050 }});
check("one tap was already a double", fired, []);
a.send("pointerdown", {{ clientX: 52, clientY: 21, timeStamp: 5200 }});
a.send("pointerup", {{ clientX: 52, clientY: 21, timeStamp: 5250 }});
check("two taps were not a double", fired, [{{ _class: "Cycle", value: "a" }}]);
fired.length = 0;
a.send("pointerdown", {{ clientX: 50, clientY: 20, timeStamp: 5400 }});
a.send("pointerup", {{ clientX: 50, clientY: 20, timeStamp: 5450 }});
check("the tap after a double counted as half of the next", fired, []);
// Far apart in time: two singles.
a.send("pointerdown", {{ clientX: 50, clientY: 20, timeStamp: 9000 }});
a.send("pointerup", {{ clientX: 50, clientY: 20, timeStamp: 9050 }});
check("two taps far apart were a double", fired, []);

// Carrying the first row below the third: pressed on the row, moved past
// the threshold, let go where the others' middles are all above it.
list.send("pointerdown", {{ target: a, clientX: 50, clientY: 20 }});
list.send("pointermove", {{ clientX: 50, clientY: 22 }});
check("a nudge marked the row as carried", "data-code-dragging" in a.attrs, false);
list.send("pointermove", {{ clientX: 50, clientY: 60 }});
check("a carried row was not marked", "data-code-dragging" in a.attrs, true);
check("the list did not take the pointer", list.captured, 1);
list.send("pointerup", {{ clientX: 50, clientY: 110 }});
check("the row's landing was not sent", fired, [{{ _class: "Move", from: 0, at: 2 }}]);
check("a row let go was still marked", "data-code-dragging" in a.attrs, false);
fired.length = 0;

// Let go between the first and the second: one place down.
list.send("pointerdown", {{ target: a, clientX: 50, clientY: 20 }});
list.send("pointermove", {{ clientX: 50, clientY: 50 }});
list.send("pointerup", {{ clientX: 50, clientY: 65 }});
check("a move of one place was not sent as one", fired, [{{ _class: "Move", from: 0, at: 1 }}]);
fired.length = 0;

// Let go where it was: nothing to say. A press with no movement: nothing.
list.send("pointerdown", {{ target: c, clientX: 50, clientY: 100 }});
list.send("pointermove", {{ clientX: 50, clientY: 130 }});
list.send("pointerup", {{ clientX: 50, clientY: 100 }});
list.send("pointerdown", {{ target: b, clientX: 50, clientY: 60 }});
list.send("pointerup", {{ clientX: 50, clientY: 60 }});
check("a row put back where it was still sent a move", fired, []);

// The browser taking the pointer for a scroll ends the drag, cleanly.
list.send("pointerdown", {{ target: a, clientX: 50, clientY: 20 }});
list.send("pointermove", {{ clientX: 50, clientY: 60 }});
list.send("pointercancel", {{}});
list.send("pointerup", {{ clientX: 50, clientY: 110 }});
check("a cancelled drag still landed", fired, []);
check("a cancelled drag left the row marked", "data-code-dragging" in a.attrs, false);

// A press that sets off sideways is a swipe on the row, not a carry: the
// list must not take the pointer, or the row never sees the release. Sent
// to both, the way a browser bubbles a pointer event.
fired.length = 0;
list.send("pointerdown", {{ target: a, clientX: 50, clientY: 20 }});
a.send("pointerdown", {{ clientX: 50, clientY: 20, timeStamp: 12000 }});
list.send("pointermove", {{ clientX: 30, clientY: 22 }});
check("a sideways start was taken as a carry", "data-code-dragging" in a.attrs, false);
check("a sideways start took the pointer", list.captured, 1);
list.send("pointermove", {{ clientX: -20, clientY: 24 }});
list.send("pointerup", {{ clientX: -20, clientY: 24, timeStamp: 12100 }});
a.send("pointerup", {{ clientX: -20, clientY: 24, timeStamp: 12100 }});
check("a swipe on a carried list's row did not reach the row", fired, [{{ _class: "Gone", id: 1, value: "a" }}]);

// A later render keeps keyed rows alive and moves them instead of replacing
// the whole list. This is the small reconciliation guarantee that keeps a
// long page stable while one item changes.
dom({{ _class: "Render", into: "body", tree: {{
  tag: "ul", on: {{ reorder: "Move" }}, children: [
    {{ tag: "li", attrs: {{ value: "c again", "data-focus-key": "c" }}, on: {{ swiperight: "Back" }} }},
    {{ tag: "li", attrs: {{ value: "a again", "data-focus-key": "a" }}, on: {{ swipeleft: {{ _class: "Gone", id: 1 }}, click: "Tapped", doubletap: "Cycle" }} }},
  ] }} }});
check("a keyed row was not kept", body.children[0].children[1] === a, true);
check("c keyed row was not moved", body.children[0].children[0] === c, true);
check("an omitted row was not removed", body.children[0].children.length, 2);
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
        "dom's gestures did not hold: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

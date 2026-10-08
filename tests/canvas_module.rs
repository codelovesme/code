//! The browser half paints bounded frames into the selected canvas and maps
//! pointer clicks back into logical scene coordinates.

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
fn a_frame_draws_and_pointer_coordinates_use_its_logical_size() {
    if !tool_exists("node") {
        eprintln!("skipped: needs node");
        return;
    }
    let half = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/canvas/page.mjs");
    let dir = std::env::temp_dir().join(format!("code-canvas-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create the probe dir");
    let probe = dir.join("probe.mjs");
    fs::write(
        &probe,
        format!(
            r##"import {{ readFileSync }} from "node:fs";
const half = new Function("return (" + readFileSync({half:?}, "utf8") + ")")();
const fired = [];
const calls = [];
class Context {{
  save() {{ calls.push(["save"]); }} restore() {{ calls.push(["restore"]); }}
  setTransform(...args) {{ calls.push(["transform", ...args]); }} clearRect(...args) {{ calls.push(["clear", ...args]); }}
  fillRect(...args) {{ calls.push(["rect", ...args]); }} strokeRect(...args) {{ calls.push(["strokeRect", ...args]); }}
  beginPath() {{ calls.push(["begin"]); }} ellipse(...args) {{ calls.push(["ellipse", ...args]); }}
  fill() {{ calls.push(["fill"]); }} stroke() {{ calls.push(["stroke"]); }}
  moveTo(...args) {{ calls.push(["move", ...args]); }} lineTo(...args) {{ calls.push(["line", ...args]); }}
  bezierCurveTo(...args) {{ calls.push(["bezier", ...args]); }} closePath() {{ calls.push(["close"]); }}
  quadraticCurveTo(...args) {{ calls.push(["quad", ...args]); }}
  transform(...args) {{ calls.push(["transform-by", ...args]); }}
  set strokeStyle(value) {{ calls.push(["strokeStyle", value]); }} set lineCap(value) {{ calls.push(["cap", value]); }}
  createRadialGradient(...args) {{
    calls.push(["radial", ...args]); return {{ radial: true, addColorStop: (...stop) => calls.push(["stop", ...stop]) }};
  }}
  fillText(...args) {{ calls.push(["text", ...args]); }} createLinearGradient(...args) {{
    calls.push(["gradient", ...args]); return {{ addColorStop: (...stop) => calls.push(["stop", ...stop]) }};
  }}
}}
const context = new Context();
const canvas = {{ width: 0, height: 0, listeners: {{}}, rect: {{ left: 20, top: 10, width: 480, height: 300 }},
  getContext: () => context,
  getBoundingClientRect() {{ return this.rect; }},
  addEventListener(name, fn) {{ (this.listeners[name] ||= []).push(fn); }},
  click(event) {{ for (const fn of this.listeners.click || []) fn(event); }}
}};
const doc = {{ querySelector: (selector) => selector === "#tank" ? canvas : null }};
globalThis.devicePixelRatio = 2;
const [, draw] = half({{ doc, fire: (particle) => fired.push(particle) }});
const check = (what, got, want) => {{
  if (JSON.stringify(got) !== JSON.stringify(want)) throw new Error(`${{what}}: got ${{JSON.stringify(got)}}, wanted ${{JSON.stringify(want)}}`);
}};
const answer = draw({{ _class: "Draw", into: "#tank", width: 960, height: 600, event: "Tapped", commands: [
  {{ op: "rect", x: 0, y: 0, width: 960, height: 600, fill: "steelblue" }},
  {{ op: "ellipse", x: 120, y: 100, rx: 20, ry: 8, rotation: 0.25, fill: "gold", stroke: "white" }},
  {{ op: "path", path: [{{ op: "move", x: 1, y: 2 }}, {{ op: "bezier", cx1: 2, cy1: 3, cx2: 4, cy2: 5, x: 6, y: 7 }}, {{ op: "close" }}], fill: "teal" }},
  {{ op: "text", text: "fish", x: 12, y: 30, size: 18, fill: "white" }}
] }});
const {{ time, ...sized }} = answer;
check("the draw result", sized, {{ _class: "CanvasResult", ok: true, width: 960, height: 600, view_width: 480, view_height: 300 }});
check("the page clock came back", typeof time === "number" && Number.isFinite(time), true);
check("the backing size follows DPR", [canvas.width, canvas.height], [960, 600]);
check("a clear and logical transform were issued", calls.some(c => c[0] === "clear") && calls.some(c => c[0] === "transform" && c[1] === 1 && c[4] === 1), true);
check("the shapes were painted", calls.some(c => c[0] === "rect") && calls.some(c => c[0] === "ellipse") && calls.some(c => c[0] === "bezier") && calls.some(c => c[0] === "text"), true);
canvas.click({{ clientX: 260, clientY: 160, pointerType: "touch" }});
check("the click was mapped to logical coordinates", fired, [{{ _class: "Tapped", x: 480, y: 300, pointer_type: "touch" }}]);
calls.length = 0;
draw({{ _class: "Draw", into: "#tank", width: 960, height: 600, commands: [
  {{ op: "ellipse", x: 50, y: 50, rx: 4, ry: 4, fill: {{ type: "radial", x0: 49, y0: 49, r0: 0, x1: 50, y1: 50, r1: 4, stops: [{{ at: 0, color: "white" }}, {{ at: 1, color: "blue" }}] }} }},
  {{ op: "path", line_cap: "butt", stroke: {{ type: "linear", x0: 0, y0: 0, x1: 0, y1: 10, stops: [{{ at: 0, color: "green" }}] }}, path: [{{ op: "move", x: 0, y: 0 }}, {{ op: "quad", cx: 3, cy: 4, x: 5, y: 6 }}] }}
] }});
check("a radial gradient was made", calls.some(c => c[0] === "radial" && c[3] === 0 && c[6] === 4), true);
check("a quadratic segment was drawn", calls.some(c => c[0] === "quad" && c[1] === 3 && c[4] === 6), true);
check("a gradient can stroke", calls.some(c => c[0] === "strokeStyle" && typeof c[1] === "object"), true);
check("a butt cap was honoured", calls.some(c => c[0] === "cap" && c[1] === "butt"), true);
check("a frame clears by default", calls.some(c => c[0] === "clear"), true);
calls.length = 0;
draw({{ _class: "Draw", into: "#tank", width: 960, height: 600, clear: false, commands: [
  {{ op: "group", transform: [2, 0, 0, 2, 100, 50], alpha: 0.5, commands: [
    {{ op: "rect", x: 1, y: 1, width: 2, height: 2, fill: "red" }},
    {{ op: "group", commands: [{{ op: "ellipse", x: 0, y: 0, rx: 1, ry: 1, fill: "red" }}] }}
  ] }}
] }});
check("clear = false paints over what is there", calls.some(c => c[0] === "clear"), false);
check("a group applies its transform", calls.some(c => c[0] === "transform-by" && c[1] === 2 && c[5] === 100 && c[6] === 50), true);
check("a group paints its children, nested ones too", calls.some(c => c[0] === "rect") && calls.some(c => c[0] === "ellipse"), true);
calls.length = 0;
const flood = Array.from({{ length: 3000 }}, () => ({{ op: "rect", x: 0, y: 0, width: 1, height: 1, fill: "red" }}));
draw({{ _class: "Draw", into: "#tank", width: 960, height: 600, commands: [{{ op: "group", commands: flood }}, {{ op: "group", commands: flood }}] }});
check("nested commands count toward the frame's limit", calls.filter(c => c[0] === "rect").length, 3998);
draw({{ _class: "Draw", into: "#tank", width: 960, height: 600, density: 1, commands: [] }});
check("a frame may name its own density", [canvas.width, canvas.height], [480, 300]);
const keepAnswer = draw({{ _class: "Keep", into: "#tank", name: "reef", commands: [{{ op: "poly", closed: true, fill: "red", xy: [0, 0, 4, 0, 4, 4] }}, {{ op: "path", stroke: "white", d: ["M", 1, 2, "Q", 3, 4, 5, 6, "C", 1, 1, 2, 2, 3, 3, "L", 7, 8, "Z"] }}] }});
check("a drawing is kept by name", keepAnswer.ok && keepAnswer.kept === 1, true);
calls.length = 0;
draw({{ _class: "Draw", into: "#tank", width: 960, height: 600, commands: [{{ op: "use", name: "reef", transform: [1, 0, 0, 1, 10, 10] }}, {{ op: "use", name: "nothing kept" }}] }});
check("a use paints what was kept, compact points and paths included",
  calls.some(c => c[0] === "line" && c[1] === 4 && c[2] === 4) && calls.some(c => c[0] === "quad" && c[1] === 3)
    && calls.some(c => c[0] === "bezier") && calls.some(c => c[0] === "line" && c[1] === 7) && calls.some(c => c[0] === "transform-by" && c[5] === 10), true);
draw({{ _class: "Keep", into: "#tank", name: "reef", commands: [] }});
calls.length = 0;
draw({{ _class: "Draw", into: "#tank", width: 960, height: 600, commands: [{{ op: "use", name: "reef" }}] }});
check("keeping nothing forgets the drawing", calls.some(c => c[0] === "fill"), false);
const invalid = draw({{ _class: "Draw", into: "#tank", width: 5000, height: 600, commands: [] }});
check("an oversized frame was refused", invalid.ok, false);
const missing = draw({{ _class: "Draw", into: "#missing", width: 100, height: 100, commands: [] }});
check("a missing canvas was reported", missing.ok, false);
"##,
        ),
    )
    .expect("write the probe");

    let output = Command::new("node")
        .arg(&probe)
        .output()
        .expect("run the probe under node");
    assert!(
        output.status.success(),
        "canvas's browser half did not draw the expected frame: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

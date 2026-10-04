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
check("the draw result", answer, {{ _class: "CanvasResult", ok: true, width: 960, height: 600 }});
check("the backing size follows DPR", [canvas.width, canvas.height], [960, 600]);
check("a clear and logical transform were issued", calls.some(c => c[0] === "clear") && calls.some(c => c[0] === "transform" && c[1] === 1 && c[4] === 1), true);
check("the shapes were painted", calls.some(c => c[0] === "rect") && calls.some(c => c[0] === "ellipse") && calls.some(c => c[0] === "bezier") && calls.some(c => c[0] === "text"), true);
canvas.click({{ clientX: 260, clientY: 160, pointerType: "touch" }});
check("the click was mapped to logical coordinates", fired, [{{ _class: "Tapped", x: 480, y: 300, pointer_type: "touch" }}]);
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

//! `media`'s page half says where the device is: `Locate` answers at once,
//! `Located` follows from the browser's fix; a person saying no is
//! `Denied`, no fix is `Unavailable`, and a browser with no geolocation at
//! all says so rather than staying quiet. Run under node against a
//! stand-in `navigator.geolocation`.

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
fn a_page_says_where_it_is_or_why_not() {
    if !tool_exists("node") {
        eprintln!("skipped: needs node");
        return;
    }
    let half = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/media/page.mjs");
    let dir = std::env::temp_dir().join(format!("code-media-locate-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create the probe dir");
    let probe = dir.join("probe.mjs");
    fs::write(
        &probe,
        format!(
            r#"import {{ readFileSync }} from "node:fs";
const half = new Function("return (" + readFileSync({half:?}, "utf8") + ")")();
const doc = {{ body: null, querySelector: () => null, createElement: () => ({{}}) }};
const fired = [];
const check = (what, got, want) => {{
  if (JSON.stringify(got) !== JSON.stringify(want)) {{
    console.error(`${{what}}: got ${{JSON.stringify(got)}}, wanted ${{JSON.stringify(want)}}`);
    process.exit(1);
  }}
}};
let next = null;
let asked = null;
Object.defineProperty(globalThis, "navigator", {{ value: {{ geolocation: {{
  getCurrentPosition(ok, no, options) {{ asked = options; next(ok, no); }}
}} }}, configurable: true }});
const [, media] = half({{ doc, fire: (p) => fired.push(p) }});

next = (ok) => ok({{ coords: {{ latitude: 50.0865, longitude: 14.4114, accuracy: 12 }}, timestamp: 1795000000123 }});
check("a locate did not answer at once", media({{ _class: "Locate", precise: true }}), {{ _class: "LocateResult", ok: true }});
check("the fix was not said", fired, [{{ _class: "Located", lat: 50.0865, lon: 14.4114, accuracy: 12, at: 1795000000 }}]);
check("precise was not asked for", asked.enableHighAccuracy, true);
check("the default timeout", asked.timeout, 15000);
fired.length = 0;

next = (ok, no) => no({{ code: 1, message: "User denied Geolocation" }});
media({{ _class: "Locate" }});
check("a refusal was not Denied", fired, [{{ _class: "Denied", device: "location", reason: "User denied Geolocation" }}]);
fired.length = 0;

next = (ok, no) => no({{ code: 3, message: "Timeout expired" }});
media({{ _class: "Locate", timeout_ms: 2000 }});
check("no fix was not Unavailable", fired, [{{ _class: "Unavailable", device: "location", reason: "Timeout expired" }}]);
check("the timeout given", asked.timeout, 2000);
fired.length = 0;

Object.defineProperty(globalThis, "navigator", {{ value: {{}}, configurable: true }});
check("no geolocation claimed to start", media({{ _class: "Locate" }}), {{ _class: "LocateResult", ok: false }});
check("no geolocation was not said", fired.map((p) => p._class + ":" + p.device), ["Unavailable:location"]);
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
        "media's Locate did not hold: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

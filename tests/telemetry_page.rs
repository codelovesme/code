//! `telemetry`'s page half keeps a page's trouble and sends it: an
//! application's `Note`, a request `net_client` never got an answer to, and
//! a script error nothing caught all land in one collector, each naming its
//! program; the same line again is counted; a batch leaves the device only
//! when the far side answers `Reported`, and stays through anything else.
//! Run under node against stand-ins for `localStorage`, `fetch` and the
//! window's events.

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
fn a_pages_trouble_is_kept_counted_and_sent() {
    if !tool_exists("node") {
        eprintln!("skipped: needs node");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let half = root.join("crates/modules/telemetry/page.mjs");
    let net = root.join("crates/modules/net_client/page.mjs");
    let dir = std::env::temp_dir().join(format!("code-telemetry-page-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create the probe dir");
    let probe = dir.join("probe.mjs");
    fs::write(
        &probe,
        format!(
            r#"import {{ readFileSync }} from "node:fs";
const load = (path) => new Function("return (" + readFileSync(path, "utf8") + ")")();
const check = (what, got, want) => {{
  if (JSON.stringify(got) !== JSON.stringify(want)) {{
    console.error(`${{what}}: got ${{JSON.stringify(got)}}, wanted ${{JSON.stringify(want)}}`);
    process.exit(1);
  }}
}};
const kept = {{}};
globalThis.localStorage = {{ getItem: (k) => kept[k] ?? null, setItem: (k, v) => {{ kept[k] = v; }} }};
const listeners = {{}};
globalThis.addEventListener = (name, fn) => {{ (listeners[name] ??= []).push(fn); }};
const sent = [];
let answer = {{ _class: "Reported" }};
globalThis.fetch = (url, init) => {{
  sent.push({{ url, body: JSON.parse(init.body) }});
  if (url === "/api/keepsake") return Promise.reject(Object.assign(new Error("aborted"), {{ name: "AbortError" }}));
  return Promise.resolve({{ json: () => Promise.resolve(answer), text: () => Promise.resolve(JSON.stringify(answer)) }});
}};
const tick = () => new Promise((r) => setTimeout(r, 0));

const half = load({half:?});
const [, shell] = half({{ program: null, fire: () => {{}} }});
const [, guest] = half({{ program: "keepsake-web", fire: () => {{}} }});

check("a Configure to another origin was taken", shell({{ _class: "Configure", url: "https://elsewhere/x", token: "t" }})._class, "Exception");
check("an empty note was taken", guest({{ _class: "Note", level: "error", message: "" }}), {{ _class: "NoteResult", ok: false }});
check("a step was not a NoteResult", guest({{ _class: "Note", message: "opened the draft" }}), {{ _class: "NoteResult", ok: true }});
check("a step was kept as trouble", guest({{ _class: "Kept" }}).items.length, 0);
guest({{ _class: "Note", level: "error", kind: "sync", message: "the upload was refused" }});
guest({{ _class: "Note", level: "error", kind: "sync", message: "the upload was refused" }});
let items = shell({{ _class: "Kept" }}).items;
check("two of the same are not one line", items.length, 1);
check("the line does not name its program", items[0].app, "keepsake-web");
check("the line is not counted twice", items[0].count, 2);
check("the trail does not hold the step", items[0].trail, ["keepsake-web: opened the draft"]);
check("the line was not kept on the device", JSON.parse(kept["code:telemetry"]).length, 1);

// A request that never came back, written by net_client without being asked.
const [, client] = load({net:?})({{ program: "keepsake-web", fire: () => {{}} }});
client({{ _class: "Config", url: "/api/keepsake" }});
client({{ _class: "Send", particle: {{ _class: "Impulse", token: "secret", particle: {{ _class: "UploadMedia", data: "private bytes" }} }}, timeout_ms: 60000 }});
await tick(); await tick();
items = shell({{ _class: "Kept" }}).items;
check("the failed request was not kept", items.length, 2);
check("the failed request is not net trouble", [items[1].kind, items[1].app, items[1].level], ["net", "keepsake-web", "warn"]);
if (!items[1].message.startsWith("Impulse/UploadMedia to /api/keepsake: no answer within 60000ms")) {{
  console.error("the failed request says: " + items[1].message); process.exit(1);
}}
if (JSON.stringify(items).includes("private bytes") || JSON.stringify(items).includes("secret")) {{
  console.error("what was said was kept"); process.exit(1);
}}

// A script error nothing caught.
listeners.error[0]({{ message: "boom", filename: "/host.mjs", lineno: 7 }});
items = shell({{ _class: "Kept" }}).items;
check("the uncaught error was not kept", [items[2].kind, items[2].message], ["uncaught", "boom (/host.mjs:7)"]);

// Sent once configured; kept through an answer that is not Reported.
answer = {{ _class: "Forbidden" }};
sent.length = 0;
check("Configure was not taken", shell({{ _class: "Configure", url: "/api/logs", token: "tok", app: "host-web" }}), {{ _class: "ConfigureResult", ok: true }});
await tick(); await tick();
check("nothing was sent to the door", sent.map((s) => [s.url, s.body._class, s.body.token, s.body.particle._class]), [["/api/logs", "Impulse", "tok", "PageReport"]]);
check("the batch was not all three", sent[0].body.particle.items.length, 3);
check("a refused batch left the device", shell({{ _class: "Kept" }}).items.length, 3);
answer = {{ _class: "Reported" }};
check("Flush did not send", shell({{ _class: "Flush" }}), {{ _class: "FlushResult", ok: true }});
await tick(); await tick();
check("a reported batch stayed", shell({{ _class: "Kept" }}).items.length, 0);
check("the device still keeps it", JSON.parse(kept["code:telemetry"]).length, 0);
check("a class it does not handle was not null", shell({{ _class: "Paste" }}), null);
process.exit(0);
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
        "telemetry's page half did not hold: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&dir);
}

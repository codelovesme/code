//! `media`'s page half can end a recording itself: with `until_silence_ms`
//! it waits for speech, then for that much quiet, and only then fires
//! `Recorded`; quiet with no speech fires nothing; `max_ms` caps it either
//! way; and `StopRecording` still ends it early. Run under node against a
//! stand-in microphone whose level the probe sets.

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
fn a_recording_ends_when_the_person_has_spoken_and_gone_quiet() {
    if !tool_exists("node") {
        eprintln!("skipped: needs node");
        return;
    }
    let half = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/media/page.mjs");
    let dir = std::env::temp_dir().join(format!("code-media-silence-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create the probe dir");
    let probe = dir.join("probe.mjs");
    fs::write(
        &probe,
        format!(
            r#"import {{ readFileSync }} from "node:fs";
const half = new Function("return (" + readFileSync({half:?}, "utf8") + ")")();

// The microphone's level, set by the test: 0 is a quiet room, 0.3 a voice.
let level = 0;
class AudioContext {{
  createAnalyser() {{
    return {{ fftSize: 0, getFloatTimeDomainData(buf) {{ buf.fill(level); }} }};
  }}
  createMediaStreamSource() {{ return {{ connect() {{}} }}; }}
  close() {{ return Promise.resolve(); }}
}}
globalThis.AudioContext = AudioContext;

const recorders = [];
class MediaRecorder {{
  constructor(stream) {{ this.stream = stream; this.mimeType = "audio/webm"; this.state = "inactive"; recorders.push(this); }}
  start() {{ this.state = "recording"; }}
  stop() {{
    if (this.state !== "recording") throw new Error("inactive");
    this.state = "inactive";
    this.ondataavailable({{ data: {{ size: 3 }} }});
    this.onstop();
  }}
}}
globalThis.MediaRecorder = MediaRecorder;
globalThis.Blob = class {{ constructor(parts, opts) {{ this.parts = parts; this.type = opts.type; }} }};
globalThis.FileReader = class {{
  readAsDataURL(blob) {{ this.result = "data:" + blob.type + ";base64,QUJD"; this.onload(); }}
}};
const stream = {{ getTracks: () => [{{ stop() {{}} }}] }};
Object.defineProperty(globalThis, "navigator", {{ get: () => ({{ mediaDevices: {{ getUserMedia: () => Promise.resolve(stream) }} }}), configurable: true }});

const fired = [];
const doc = {{ body: null, querySelector: () => null, createElement: () => ({{}}) }};
const [, media] = half({{ doc, fire: (p) => fired.push(p) }});
const check = (what, got, want) => {{
  if (JSON.stringify(got) !== JSON.stringify(want)) {{
    console.error(`${{what}}: got ${{JSON.stringify(got)}}, wanted ${{JSON.stringify(want)}}`);
    process.exit(1);
  }}
}};
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const recorded = () => fired.filter((p) => p._class === "Recorded");

// Quiet with no speech: nothing, however long.
media({{ _class: "Record", until_silence_ms: 200, max_ms: 5000 }});
await wait(50);
level = 0;
await wait(600);
check("a quiet room fired a recording", recorded(), []);

// Speech, then quiet: the recording ends itself.
level = 0.3;
await wait(300);
level = 0;
await wait(500);
check("speech then quiet did not fire the recording", recorded().length, 1);
check("the recording did not carry the bytes", recorded()[0].audio_base64, "QUJD");

// The cap ends a recording whether or not it went quiet.
fired.length = 0;
media({{ _class: "Record", until_silence_ms: 200, max_ms: 400 }});
await wait(50);
level = 0.3;
await wait(700);
check("the cap did not end the recording", recorded().length, 1);

// Stopping early still works, and without `until_silence_ms` nothing changes.
fired.length = 0;
media({{ _class: "Record", until_silence_ms: 200 }});
await wait(50);
check("a stop did not answer success", media({{ _class: "StopRecording" }}), {{ _class: "StopResult", ok: true }});
await wait(20);
check("an early stop did not fire the recording", recorded().length, 1);
fired.length = 0;
media({{ _class: "Record" }});
await wait(50);
level = 0.3;
await wait(300);
level = 0;
await wait(500);
check("a plain recording ended itself", recorded(), []);
media({{ _class: "StopRecording" }});
await wait(20);
check("a plain recording did not stop on request", recorded().length, 1);
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
        "media's silence did not hold: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

//! `media`'s page half plays a sound the program holds and says when it
//! ended: `Play` answers at once, `Played` follows from the element's own
//! `ended`; a second `Play` replaces the first without letting it speak;
//! `StopPlaying` is quiet. Run under node against a stand-in `Audio` — the
//! half asks it for nothing a fake cannot answer.

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
fn a_sound_plays_and_says_when_it_ended() {
    if !tool_exists("node") {
        eprintln!("skipped: needs node");
        return;
    }
    let half = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/media/page.mjs");
    let dir = std::env::temp_dir().join(format!("code-media-speaker-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create the probe dir");
    let probe = dir.join("probe.mjs");
    fs::write(
        &probe,
        format!(
            r#"import {{ readFileSync }} from "node:fs";
const half = new Function("return (" + readFileSync({half:?}, "utf8") + ")")();
const made = [];
class Audio {{
  constructor(src) {{ this.src = src; this.paused = false; made.push(this); }}
  play() {{ return Promise.resolve(); }}
  pause() {{ this.paused = true; }}
}}
globalThis.Audio = Audio;
const doc = {{ body: null, querySelector: () => null, createElement: () => ({{}}) }};
const fired = [];
const [, media] = half({{ doc, fire: (p) => fired.push(p) }});
const check = (what, got, want) => {{
  if (JSON.stringify(got) !== JSON.stringify(want)) {{
    console.error(`${{what}}: got ${{JSON.stringify(got)}}, wanted ${{JSON.stringify(want)}}`);
    process.exit(1);
  }}
}};

check("stopping with nothing playing claimed success", media({{ _class: "StopPlaying" }}), {{ _class: "StopResult", ok: false }});
check("playing nothing claimed success", media({{ _class: "Play", audio_base64: "" }}), {{ _class: "PlayResult", ok: false }});

check("a play did not answer at once", media({{ _class: "Play", audio_base64: "UklGRg==", format: "wav" }}), {{ _class: "PlayResult", ok: true }});
check("the sound was not the bytes given", made[0].src, "data:audio/wav;base64,UklGRg==");
made[0].onended();
check("the end of the sound was not said", fired, [{{ _class: "Played" }}]);
fired.length = 0;

// A second sound replaces the first, which is silenced and says nothing.
media({{ _class: "Play", audio_base64: "AAAA", format: "mp3" }});
media({{ _class: "Play", audio_base64: "BBBB", format: "mp3" }});
check("the replaced sound was left playing", made[1].paused, true);
check("the replaced sound still ended", [made[1].onended, fired], [null, []]);
check("a stop did not answer success", media({{ _class: "StopPlaying" }}), {{ _class: "StopResult", ok: true }});
check("a stopped sound was left playing", made[2].paused, true);
check("a stopped sound spoke", fired, []);
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
        "media's speaker did not hold: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

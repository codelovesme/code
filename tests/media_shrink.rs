//! `media`'s `Shrink`: a picture the program holds is made small — the
//! longer side cut to 400 by default or to `max_side`, a small one kept at
//! its size, the `ref` handed back, and a picture the browser cannot read
//! answered `Unshrinkable`. Run under node against a stand-in decoder and
//! canvas; the probe is `media_shrink_probe.mjs`.

use std::path::Path;
use std::process::Command;

#[test]
fn a_picture_is_made_small_and_a_non_picture_is_refused() {
    if Command::new("node")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        eprintln!("skipped: needs node");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("node")
        .arg(root.join("tests/media_shrink_probe.mjs"))
        .arg(root.join("crates/modules/media/page.mjs"))
        .output()
        .expect("run the probe under node");
    assert!(
        output.status.success(),
        "media's Shrink did not hold: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

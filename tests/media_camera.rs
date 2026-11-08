//! `media`'s camera: it opens at the most pixels the camera has (not the
//! browser's 640×480), says which cameras there are so one can be picked,
//! zooms within the camera's range, keeps the photo's pixels when asked, and
//! turns the photo the way the phone was held. Run under node against a
//! stand-in phone; the probe is `media_camera_probe.mjs`.

use std::path::Path;
use std::process::Command;

#[test]
fn the_camera_is_picked_sharp_and_the_right_way_up() {
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
        .arg(root.join("tests/media_camera_probe.mjs"))
        .arg(root.join("crates/modules/media/page.mjs"))
        .output()
        .expect("run the probe under node");
    assert!(
        output.status.success(),
        "media's camera did not hold: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

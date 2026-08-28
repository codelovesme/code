//! `templates/module/` has to actually work.
//!
//! It is the first thing someone writing a module copies, so a broken one is
//! worse than none — and it is exactly the kind of file that rots silently,
//! since nothing else in the tree references it. The ABI it is written
//! against changes here; the template does not follow along on its own.
//!
//! So this builds it and runs its fixture through both output modes, which is
//! the same bar every first-party module is held to.
//!
//! The one substitution: the template depends on `code-native = "1"` from
//! crates.io, which is right for the person copying it and wrong for a test
//! that must not need the network or a publish. It is rewritten to a path
//! dependency on this repo's own copy — which also makes the test sharper
//! than the published version would be, since it catches a template that
//! stopped matching the ABI as it stands *now* rather than as it shipped.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Copies the template somewhere writable, pointing `code-native` at this
/// repo. Returns the copy's root.
fn staged(dir: &Path) -> PathBuf {
    let src = repo_root().join("templates/module");
    let dst = dir.join("module");
    copy_tree(&src, &dst);

    let manifest = dst.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).expect("read template Cargo.toml");
    let native = repo_root().join("crates/code-native");
    let patched = text.replace(
        "code-native = \"1\"",
        &format!(
            "code-native = {{ path = {:?} }}",
            native.display().to_string()
        ),
    );
    assert_ne!(
        text, patched,
        "the template's `code-native` dependency line changed shape; this test \
         no longer knows how to point it at the in-tree crate"
    );
    std::fs::write(&manifest, patched).expect("write patched Cargo.toml");
    dst
}

fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create staging directory");
    for entry in std::fs::read_dir(src).expect("read template") {
        let entry = entry.expect("template entry");
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to);
        } else {
            std::fs::copy(&from, &to).expect("copy template file");
        }
    }
}

#[test]
fn the_module_template_builds_and_its_fixture_passes_in_both_modes() {
    let dir = std::env::temp_dir().join(format!("code-template-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test directory");
    let module = staged(&dir);

    let status = Command::new(env!("CARGO"))
        .args(["build", "--release"])
        .current_dir(&module)
        .status()
        .expect("run cargo for the template");
    assert!(status.success(), "the module template does not build");

    // Beside the fixture, under the name the fixture's `link` uses.
    let built = module.join("target/release/libgreet.so");
    let linked = module.join("tests/greet.so");
    std::fs::copy(&built, &linked).expect("stage the built module beside its fixture");

    let fixture = module.join("tests/greet.code");
    let interpreted = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("run `code run`");
    assert!(
        interpreted.status.success(),
        "the template's fixture failed under `code run`: {}",
        String::from_utf8_lossy(&interpreted.stderr)
    );

    let exe = module.join("greet-fixture");
    code::compile_file(&fixture, code::BuildTarget::Exe, &exe, false)
        .expect("the template's fixture should compile");
    let compiled = Command::new(&exe)
        // The same leak check the fixture harness runs everything under: a
        // module that loses a reference shows up here and nowhere else.
        .env("CODE_CHECK_LEAKS", "1")
        .output()
        .expect("run the compiled fixture");
    assert!(
        compiled.status.success(),
        "the template's fixture failed under `code build`: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

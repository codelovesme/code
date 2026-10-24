//! A module that keeps no state is loaded once, however often it is linked
//! (`code_abi.h` item 12, ticket 177); one that does not say so still gets
//! its own copy per link (`link_names_one_module.code` keeps that half).
//!
//! Counted from the outside: the program asks a shell how many in-memory
//! module images its own process holds. Two links of `process`, which makes
//! no such declaration, are two images; two links of `strings`, which does,
//! are none — it is the file itself, opened once.
#![cfg(feature = "llvm")]
#[path = "support/modules.rs"]
mod modules;
use std::{fs, process::Command};

const PROGRAM: &str = r#"
link "native_modules/strings.so" as a
link "native_modules/strings.so" as b
link "native_modules/process.so" as p1
link "native_modules/process.so" as p2

emit Length { value = "abc" } to a get x
assert x.value = 3
emit Length { value = "ab" } to b get y
assert y.value = 2

emit Run { command = "sh", args = ["-c", "ls -l /proc/\$PPID/fd | grep -c memfd:code-module"] } to p1 get r
assert r.success
emit Trim { value = r.stdout } to b get images
assert images.value = "2"
"#;

#[test]
fn a_stateless_module_is_one_instance_and_others_one_per_link() {
    let dir = std::env::temp_dir().join(format!("code-stateless-{}", std::process::id()));
    fs::create_dir_all(dir.join("native_modules")).unwrap();
    for name in ["strings", "process"] {
        fs::copy(
            modules::build_so(name),
            dir.join(format!("native_modules/{name}.so")),
        )
        .unwrap();
    }
    let source = dir.join("program.code");
    fs::write(&source, PROGRAM).unwrap();

    let interpreted = Command::new(env!("CARGO_BIN_EXE_code"))
        .args(["run", "--interpret"])
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        interpreted.status.success(),
        "interpreted: {}",
        String::from_utf8_lossy(&interpreted.stderr)
    );

    for release in [false, true] {
        let bin = dir.join(if release { "release" } else { "dev" });
        code::compile_file(&source, code::BuildTarget::Exe, &bin, release).unwrap();
        let out = Command::new(&bin).output().unwrap();
        assert!(
            out.status.success(),
            "compiled (release {release}): {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// A module with state, opened once and instantiated per link (`code_abi.h`
/// item 13, ticket 178): two links of `jwt` are no images of their own, and
/// their settings still stay apart — the second `Config` must not change
/// what the first signs with (`link_names_one_module.code`'s case).
const INSTANCES: &str = r#"
link "native_modules/jwt.so" as first
link "native_modules/jwt.so" as second
link "native_modules/process.so" as p
link "native_modules/strings.so" as s

emit Config { secret = "first-secret" } to first get a
assert a.ok
emit Config { secret = "second-secret" } to second get b
assert b.ok

emit Sign { sub = "someone" } to first get one
emit Sign { sub = "someone" } to second get two
assert not one.token = two.token
emit Decode { token = one.token } to first get mine
assert mine.valid
emit Decode { token = two.token } to first get theirs
assert not theirs.valid
emit Decode { token = two.token } to second get hers
assert hers.valid

emit Run { command = "sh", args = ["-c", "ls -l /proc/\$PPID/fd | grep -c memfd:code-module"] } to p get r
emit Trim { value = r.stdout } to s get images
assert images.value = "1"
"#;

#[test]
fn a_module_with_instances_is_one_image_with_settings_per_link() {
    let dir = std::env::temp_dir().join(format!("code-instances-{}", std::process::id()));
    fs::create_dir_all(dir.join("native_modules")).unwrap();
    for name in ["jwt", "process", "strings"] {
        fs::copy(
            modules::build_so(name),
            dir.join(format!("native_modules/{name}.so")),
        )
        .unwrap();
    }
    let source = dir.join("program.code");
    fs::write(&source, INSTANCES).unwrap();

    let interpreted = Command::new(env!("CARGO_BIN_EXE_code"))
        .args(["run", "--interpret"])
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        interpreted.status.success(),
        "interpreted: {}",
        String::from_utf8_lossy(&interpreted.stderr)
    );
    for release in [false, true] {
        let bin = dir.join(if release { "release" } else { "dev" });
        code::compile_file(&source, code::BuildTarget::Exe, &bin, release).unwrap();
        let out = Command::new(&bin).output().unwrap();
        assert!(
            out.status.success(),
            "compiled (release {release}): {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

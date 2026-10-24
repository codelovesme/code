//! Real first-party modules linked into one program (ticket 141).
//!
//! Each archive is built the way a release builds it,
//! `scripts/build-module-static.sh`: a `staticlib` with no runtime of its
//! own, its exports renamed `<module>_code_module_*` and everything else
//! kept private, so several Rust modules — each with its own copy of the
//! standard library — link into one executable without a clash.
//!
//! Covered here: two archives side by side (math, strings); a module with
//! state linked under two names keeping both settings (jwt, through an
//! instance per alias — `code_abi.h` item 13); a module that pushes from a
//! thread of its own and holds the program open until it has (timer); and
//! the refusal of a module with state but no instances under a second name
//! (process). The executables need no `.so` beside them: the modules are in
//! them.
#![cfg(feature = "llvm")]
#[path = "support/modules.rs"]
mod modules;
use std::{fs, path::Path, process::Command};

fn archive(name: &str, dir: &Path) {
    let status = Command::new(modules::repo().join("scripts/build-module-static.sh"))
        .arg(name)
        .arg(dir)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "building {name}.a: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    let built = dir.join(format!("{name}-linux-x86_64.a"));
    fs::rename(&built, dir.join(format!("{name}.a"))).unwrap();
    fs::rename(
        dir.join(format!("{name}-linux-x86_64.a.libs")),
        dir.join(format!("{name}.a.libs")),
    )
    .unwrap();
}

const PROGRAM: &str = r#"
link "math.a" as math
link "strings.a" as text
link "strings.a" as again
link "jwt.a" as first
link "jwt.a" as second
link "timer.a" as clock
link "console.a" as term

emit Floor { value = 3.7 } to math get f
assert f.value = 3
emit Upper { value = "abc" } to text get u
assert u.value = "ABC"
emit Length { value = "héllo" } to again get n
assert n.value = 5

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
emit Sign { } to first get bad
assert bad ∈ Exception
emit Nothing { } to first get unknown
assert unknown = null

Refresh { what } =>
    emit Print { value = "fired $what" } to term
emit Delay { ms = 20, then = { _class = "Refresh", what = "prices" } } to clock get d
emit Print { value = "waiting" } to term
"#;

#[test]
fn real_modules_link_into_one_program() {
    let dir = std::env::temp_dir().join(format!("code-static-modules-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for name in ["math", "strings", "jwt", "timer", "console", "process"] {
        archive(name, &dir);
    }
    let source = dir.join("program.code");
    fs::write(&source, PROGRAM).unwrap();
    for release in [false, true] {
        let bin = dir.join(if release { "release" } else { "dev" });
        code::compile_file(&source, code::BuildTarget::Exe, &bin, release)
            .unwrap_or_else(|e| panic!("build (release {release}): {e}"));
        let out = Command::new(&bin).output().unwrap();
        assert!(
            out.status.success(),
            "run (release {release}): {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "waiting\nfired prices\n",
            "the timer's push arrives before the program ends"
        );
    }
    // What made that run possible: no module `.so` anywhere the program
    // could have opened one.
    let shared: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "so"))
        .collect();
    assert!(shared.is_empty(), "no .so beside the program: {shared:?}");

    let twice = dir.join("twice.code");
    fs::write(
        &twice,
        "link \"process.a\" as p1\nlink \"process.a\" as p2\n",
    )
    .unwrap();
    let refused = code::compile_file(&twice, code::BuildTarget::Exe, &dir.join("twice"), false)
        .expect_err("a module with state and no instances, linked in under two names");
    assert!(
        refused.contains("already linked under another name"),
        "{refused}"
    );
}

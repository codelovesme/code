//! `code build --target ...` — phase 1 of `docs/todo/build-targets.md`.
//!
//! The fixture suite (`run_language_tests.rs`) covers *behaviour* under the
//! default `Exe` target; this file covers *container shape*: each target
//! produces the artifact its flag promises, and the artifact is usable —
//! a `.so` the dynamic loader actually accepts, a `.a` holding exactly the
//! program object, and a wasm module that Node can instantiate and run.

#![cfg(feature = "llvm")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(name)
}

/// One private directory per test; the tag distinguishes tests within this
/// process, the pid distinguishes processes (the same convention as
/// `concurrent_builds.rs`).
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("code-build-target-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    dir
}

#[test]
fn shared_target_produces_a_loadable_library() {
    let dir = temp_dir("shared");
    let out = dir.join("libarith.so");
    code::compile_file(
        &fixture("arithmetic_basic.code"),
        code::BuildTarget::Shared,
        &out,
        false,
    )
    .expect("build --target shared");
    assert!(out.is_file(), "expected a .so at {}", out.display());

    // A `.so` whose only entry point is `main` is close to useless — the
    // real acceptance bar is that the dynamic loader accepts it: every
    // symbol resolved, any initializers run. `ctypes.CDLL` does exactly
    // that with nothing beyond the stdlib; where python3 is unavailable
    // the existence check above still stands.
    let probe = Command::new("python3")
        .arg("-c")
        .arg(format!("import ctypes; ctypes.CDLL({out:?})"))
        .output();
    match probe {
        Ok(output) if output.status.success() => {}
        Ok(output) => panic!(
            "built .so failed to load: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(e) => eprintln!("note: python3 unavailable ({e}); skipped load check"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn static_target_produces_an_archive_holding_only_the_program_object() {
    let dir = temp_dir("static");
    let out = dir.join("libarith.a");
    code::compile_file(
        &fixture("arithmetic_basic.code"),
        code::BuildTarget::Static,
        &out,
        false,
    )
    .expect("build --target static");

    // `ar t` lists members. Exactly one is expected: the program object.
    // No runtime — `Static` deliberately never links one in (consumers of
    // the archive supply their own), so a second member would mean the
    // link step leaked into the archive step.
    let listing = Command::new("ar")
        .arg("t")
        .arg(&out)
        .output()
        .expect("run ar t");
    assert!(listing.status.success(), "ar t failed on {}", out.display());
    let members = String::from_utf8(listing.stdout).expect("ar t output is utf-8");
    assert_eq!(
        members.lines().collect::<Vec<_>>(),
        vec!["program.o"],
        "unexpected archive contents: {members}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn wasm_target_produces_a_runnable_module() {
    let dir = temp_dir("wasm");
    let out = dir.join("arith.wasm");
    code::compile_file(
        &fixture("arithmetic_basic.code"),
        code::BuildTarget::Wasm,
        &out,
        false,
    )
    .expect("build --target wasm");
    assert!(out.is_file(), "expected a wasm file at {}", out.display());

    let probe = dir.join("run-wasm.mjs");
    fs::write(
        &probe,
        format!(
            "import {{ readFileSync }} from 'node:fs';\n\
             const bytes = readFileSync({out:?});\n\
             const {{ instance }} = await WebAssembly.instantiate(bytes, {{\n\
               env: {{\n\
                 code_host_error(ptr, len) {{ throw new Error(`wasm error ${{ptr}} ${{len}}`); }},\n\
                 code_host_now() {{ return Date.now() / 1000; }}\n\
               }}\n\
             }});\n\
             if (typeof instance.exports.main !== 'function') throw new Error('main was not exported');\n\
             if (instance.exports.main() !== 0) throw new Error('main returned an error');\n"
        ),
    )
    .expect("write wasm probe");
    let output = Command::new("node")
        .arg(&probe)
        .output()
        .expect("run wasm under node");
    assert!(
        output.status.success(),
        "node could not run wasm: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cli_default_output_name_follows_the_target() {
    let dir = temp_dir("cli");
    // Copy the fixture in rather than pointing at tests/: the default
    // output name derives from the input's file stem, and building beside
    // the source tree would leave an artifact among the fixtures.
    let src = dir.join("arith.code");
    fs::copy(fixture("arithmetic_basic.code"), &src).expect("copy fixture");

    // File first, flags after — the same shape as `-o` (see `main.rs`).
    let status = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("build")
        .arg(&src)
        .args(["--target", "shared"])
        .current_dir(&dir)
        .status()
        .expect("spawn code build");
    assert!(status.success(), "code build --target shared failed");
    assert!(
        dir.join("libarith.so").is_file(),
        "expected libarith.so alongside the source in {}",
        dir.display()
    );

    // An unparseable target value is a usage error, reported as such.
    let output = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("build")
        .arg(&src)
        .args(["--target", "bogus"])
        .current_dir(&dir)
        .output()
        .expect("spawn code build");
    assert!(!output.status.success(), "bogus target must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown target 'bogus'"),
        "usage error should name the bad value, got: {stderr}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// `--release` is the only knob in `code build` that changes nothing about
/// what a program *means*, which makes it the easy one to break silently:
/// drop the plumbing anywhere between the CLI flag and
/// `create_target_machine` and every test still passes. So this asserts the
/// two settings actually produce different artifacts — and that the
/// optimized one still runs, since `-O2` is the path no other test in the
/// suite exercises now that the default is `-O0`.
#[test]
fn release_optimizes_and_the_default_does_not() {
    let dir = temp_dir("release");
    let source = fixture("object_merge.code");

    let dev = dir.join("dev");
    let release = dir.join("release");
    code::compile_file(&source, code::BuildTarget::Exe, &dev, false).expect("build (default)");
    code::compile_file(&source, code::BuildTarget::Exe, &release, true).expect("build --release");

    let dev_size = fs::metadata(&dev).expect("stat default build").len();
    let release_size = fs::metadata(&release).expect("stat release build").len();
    assert_ne!(
        dev_size, release_size,
        "--release produced a byte-identical artifact ({dev_size} bytes) — \
         the flag is not reaching the target machine"
    );

    // The fixture is a wall of asserts, so a non-zero exit means the
    // optimizer changed the program's meaning.
    let status = Command::new(&release).status().expect("run release build");
    assert!(status.success(), "release build failed its own assertions");

    // And the CLI half: the flag is spelled the way the usage string
    // promises, and does not fall through to `unknown argument`.
    let output = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("build")
        .arg(&source)
        .arg("--release")
        .args(["-o", &dir.join("cli").display().to_string()])
        .output()
        .expect("spawn code build --release");
    assert!(
        output.status.success(),
        "code build --release failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = fs::remove_dir_all(&dir);
}

//! `--target shared|static` produces a module another `.code` program can
//! link — the "format determines content" decision in
//! `docs/todo/build-targets.md`.
//!
//! Everything else about those two targets is covered by
//! `tests/build_targets.rs`, which checks the *container*: a `.so` the
//! dynamic loader accepts, a `.a` holding one member. This file checks the
//! *content*, and the only honest way to do that is to be a consumer: build a
//! module from `.code`, link it from another `.code` program, and let the
//! program's own asserts say whether the values arrived. A module nothing
//! links proves nothing.
//!
//! Both output modes wherever both apply. A `.so` is reachable from `code
//! run` and `code build`; a `.a` only links under `code build` (the
//! interpreter refuses a `Static` link — see `buildonly_` in
//! `tests/run_language_tests.rs`), so its half is compiled only.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A module worth linking: values of every kind that matters here, a private
/// `let` a handler reads, and a handler that answers.
///
/// `items` and `joined` are the point of the fixture rather than decoration.
/// Both own a heap block, and until they did, nothing ever asked whether a
/// module's exported values are still alive when the host reads them — every
/// existing native module exports numbers and string *literals*, which own
/// nothing. `greeting` is deliberately not exported: a handler naming it is
/// what proves a library keeps its whole top-level scope, not just the part
/// it advertises.
const MODULE_SOURCE: &str = r#"export let items = [1, 2, 3]
export let joined = "x" + "y"
export let n = 42

let greeting = "hello "

Greet { who } => {
    return Reply { text = greeting + who }
}
"#;

/// What a consumer asserts about the module above, whichever way it linked
/// it. `{ext}` is the artifact's extension.
fn consumer_source(ext: &str) -> String {
    format!(
        r#"link "lib.{ext}" as m
assert m.items = [1, 2, 3]
assert m.joined = "xy"
assert m.n = 42
assert m.greeting = null
emit Greet {{ who = "ada" }} to m get r
assert r.text = "hello ada"
"#
    )
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("code-lib-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    dir
}

/// Writes `source` into `dir` and builds it for `target`, returning the
/// artifact.
fn build(dir: &Path, name: &str, source: &str, target: code::BuildTarget, out: &str) -> PathBuf {
    let path = dir.join(format!("{name}.code"));
    fs::write(&path, source).expect("write source");
    let artifact = dir.join(out);
    code::compile_file(&path, target, &artifact, false).unwrap_or_else(|e| {
        panic!("build {name} for {target:?}: {e}");
    });
    artifact
}

/// Compiles `dir/main.code` and runs it, returning the exit status. The
/// program asserts for itself, so success is the whole result.
fn run_compiled(dir: &Path, leak_check: bool) -> std::process::Output {
    let exe = dir.join("main");
    code::compile_file(&dir.join("main.code"), code::BuildTarget::Exe, &exe, false)
        .expect("compile the consumer");
    let mut cmd = Command::new(&exe);
    cmd.current_dir(dir);
    if leak_check {
        cmd.env("CODE_CHECK_LEAKS", "1");
    }
    cmd.output().expect("run the compiled consumer")
}

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A `.so` built from `.code` is a module: its values arrive, its handlers
/// answer, and both output modes agree.
///
/// The leak check runs on the compiled half. A `.so` carries its own runtime,
/// so the module's permanent values are counted by *its* `live_blocks` and
/// never by the host's — the host's copies are the only thing the check here
/// can see, and they have to balance.
#[test]
fn a_shared_library_built_from_code_is_linkable_from_code() {
    let dir = temp_dir("shared");
    build(
        &dir,
        "lib",
        MODULE_SOURCE,
        code::BuildTarget::Shared,
        "lib.so",
    );
    fs::write(dir.join("main.code"), consumer_source("so")).expect("write consumer");

    let interpreted = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("run")
        .arg("main.code")
        .current_dir(&dir)
        .output()
        .expect("spawn code run");
    assert!(
        interpreted.status.success(),
        "code run rejected the .so module:\n{}",
        stderr_of(&interpreted)
    );

    let compiled = run_compiled(&dir, true);
    assert!(
        compiled.status.success(),
        "the compiled consumer rejected the .so module:\n{}",
        stderr_of(&compiled)
    );

    let _ = fs::remove_dir_all(&dir);
}

/// The same module as a `.a`, linked into a compiled program.
///
/// No leak check, and the reason is the `.a` contract rather than anything
/// this build does: a static module shares the host's *one* runtime, and
/// `code_abi.h` says its exported values stay valid "for the module's whole
/// lifetime" — so the two heap blocks behind `items` and `joined` are still
/// held when the host's `main` ends, and `code_check_leaks` counts them. A C
/// module meets the same requirement with `static` storage and the same
/// visible result; `tests/native_modules/test_math_static` only avoids it by
/// exporting a single number. Documented in
/// `docs/todo/build-targets.md`.
#[test]
fn a_static_library_built_from_code_is_linkable_from_code() {
    let dir = temp_dir("static");
    build(
        &dir,
        "lib",
        MODULE_SOURCE,
        code::BuildTarget::Static,
        "lib.a",
    );
    fs::write(dir.join("main.code"), consumer_source("a")).expect("write consumer");

    let compiled = run_compiled(&dir, false);
    assert!(
        compiled.status.success(),
        "the compiled consumer rejected the .a module:\n{}",
        stderr_of(&compiled)
    );

    let _ = fs::remove_dir_all(&dir);
}

/// An archive exports its three ABI entry points and nothing else.
///
/// This is the one property a consumer cannot demonstrate on its own, because
/// its failure is a *link* error in a program that has two of something. Both
/// sides of a static link generate the same internal names — `_code_init`,
/// `_code_dispatch_this`, `_code_slot_0_num` — so with those left global the
/// archive collides with its host, and two archives collide with each other.
/// It is also what `loader.rs` depends on when it reads the prefix back out:
/// exactly one symbol may end in `_code_module_dispatch`.
#[test]
fn an_archive_exports_only_its_abi_entry_points() {
    let dir = temp_dir("symbols");
    let archive = build(
        &dir,
        "lib",
        MODULE_SOURCE,
        code::BuildTarget::Static,
        "lib.a",
    );

    let listing = Command::new("nm")
        .args(["--defined-only", "-g"])
        .arg(&archive)
        .output()
        .expect("run nm");
    assert!(listing.status.success(), "nm failed on the archive");
    let mut exported: Vec<String> = String::from_utf8_lossy(&listing.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().last())
        .filter(|name| !name.ends_with(".o:"))
        .map(str::to_owned)
        .collect();
    exported.sort();

    assert_eq!(
        exported,
        vec![
            "lib_code_module_abi_version".to_string(),
            "lib_code_module_dispatch".to_string(),
            "lib_code_module_vars".to_string(),
        ],
        "an archive's global symbols are not just its ABI entry points — \
         anything else here collides with the host it is linked into"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// A module with nothing to export omits `code_module_vars` entirely, which
/// the ABI allows ("a module that does not export it simply has no exported
/// variables") — and the alias then reads as an empty object rather than
/// failing to link.
#[test]
fn a_module_with_no_exports_omits_the_vars_entry_point() {
    let dir = temp_dir("no-vars");
    let archive = build(
        &dir,
        "lib",
        "Ping { } => {\n    return Pong { ok = true }\n}\n",
        code::BuildTarget::Static,
        "lib.a",
    );

    let listing = Command::new("nm")
        .args(["--defined-only", "-g"])
        .arg(&archive)
        .output()
        .expect("run nm");
    let text = String::from_utf8_lossy(&listing.stdout);
    assert!(
        !text.contains("code_module_vars"),
        "a module with no `export let` should not export code_module_vars:\n{text}"
    );

    fs::write(
        dir.join("main.code"),
        "link \"lib.a\" as m\nassert m.anything = null\nemit Ping { } to m get r\nassert r.ok\n",
    )
    .expect("write consumer");
    let compiled = run_compiled(&dir, false);
    assert!(
        compiled.status.success(),
        "a handlers-only archive did not link:\n{}",
        stderr_of(&compiled)
    );

    let _ = fs::remove_dir_all(&dir);
}

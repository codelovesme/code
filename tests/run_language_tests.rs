//! Discovers and runs every `tests/*.code` fixture through BOTH output
//! modes — `code run` (interpret, as a subprocess) and `code build`
//! (LLVM compile + link + execute) — since the language is meant to run
//! every feature identically either way (see memory `new-language-rewrite`).
//! This file is wiring only — the tests themselves are the `.code` files.
//!
//! Pass criterion for a plain `foo.code`: both modes must succeed, and the
//! compiled binary must leak nothing (it runs with `CODE_CHECK_LEAKS=1`, so
//! the runtime aborts at exit if any heap block survives — see
//! `check_compile`). Correctness lives in the fixtures themselves: each one
//! `assert`s the values it cares about, in both modes. Programs are
//! otherwise silent — there is no bindings dump anymore, so a module's
//! `Print` writes straight to stdout and there is nothing to compare across
//! backends; a fixture that prints simply prints, in both modes.
//! For a `fail_foo.code`: both modes must produce an error — for the
//! interpreter that's the `code run` subprocess exiting non-zero (the
//! interpret check runs the real binary in a child process, because a
//! linked module's fatal error takes the *host* process down with it —
//! see `docs/todo/native-module-linking.md`; a subprocess turns that into
//! a capturable exit code instead of killing this harness); for the
//! compiler it's either a compile-time error (`compile_source` returning
//! `Err`, e.g. a parse error or `verify_defined`'s undefined-variable
//! check) OR the compiled binary itself exiting non-zero at runtime (e.g. a
//! type mismatch or division by zero — those operand types aren't known
//! until the program actually runs, so the compiled binary has to detect
//! and report them itself; see `runtime.c`'s `code_runtime_error`).
//!
//! A `buildonly_foo.code` is the one deliberate exception to "every feature
//! behaves identically in both modes": a `.a`-linked native module (see
//! `docs/todo/native-module-linking.md`) only works under `code build` —
//! there is no `dlopen` for a static archive — so these must *fail* under
//! `code run` and succeed (with a clean exit, leak check included) under
//! `code build`. There is no interpreted run at all for these.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn code_fixtures_run_as_expected() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let tmp_dir = std::env::temp_dir().join("code-compiler-tests");
    fs::create_dir_all(&tmp_dir).expect("create temp dir for compiled fixtures");

    build_native_dynamic_test_modules(&dir);
    build_native_static_test_modules(&dir);

    let mut failures = Vec::new();
    let mut checked = 0;

    for entry in fs::read_dir(&dir).expect("read tests/ directory") {
        let path = entry.expect("read tests/ directory entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("code") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let expect = if name.starts_with("fail_") {
            Expect::Fail
        } else if name.starts_with("buildonly_") {
            Expect::BuildOnly
        } else {
            Expect::Succeed
        };

        // Both modes take the fixture's *path*, not its text: `link`
        // resolves relative to the linking file, so `tests/modules/*.code`
        // is only reachable from a caller that knows where the fixture is.
        // Those module files live in a subdirectory and so are never picked
        // up as fixtures in their own right by the glob above.
        check_interpret(&name, &path, expect, &mut failures);
        check_compile(&name, &path, expect, &tmp_dir, &mut failures);
        checked += 1;
    }

    assert!(checked > 0, "no .code fixtures found in {}", dir.display());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// What a fixture's filename prefix (`fail_`/`buildonly_`/none) says about
/// how the two output modes should behave — see this file's top comment.
#[derive(Clone, Copy, PartialEq)]
enum Expect {
    Succeed,
    Fail,
    BuildOnly,
}

/// Compiles each dynamic native module into the `.so` the
/// `native_link_*`/`fail_native_link_*`, `terminal_*`, `strings_*`,
/// `math_*`, `json_*`, `json_store*`, `crypto_*`, `jwt_*`, `markdown_*`, `fs_*`, `process_*`, `git_*`, `mailer_*`, `oauth_*`, `mongodb_*`, `blob_storage_*`, `cloud_drive_*`, `localai_*`, `*_mock_*`, `net_*`, and `http_client_*` fixtures `link` — checked into git as source, not
/// as a binary
/// (see `.gitignore`), so it has to be built fresh here before any fixture
/// that needs it can run either mode. Sources live next to their consumers:
/// `test_math` is a pure test double (stays in `tests/native_modules/`),
/// while `terminal`, `strings`, `math`, `env`, `json`, and `http_client` are
/// real first-party modules
/// that happen to be exercised by fixtures (their canonical homes are under
/// `crates/modules/`, where the release CI builds them from). The C modules
/// go straight through `cc`; the rest are Rust-on-`code-native` modules, so
/// they get a `cargo build` instead — same output location, same stem, the
/// fixtures cannot tell the difference. `http_client` is much the slowest to
/// build cold (it pulls ureq and rustls); nothing here needs a network,
/// though — its fixtures only ever talk to a refused port on loopback.
fn build_native_dynamic_test_modules(tests_dir: &Path) {
    let modules_dir = tests_dir.join("native_modules");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    // The Rust modules: `cargo build` inside each standalone workspace, then
    // move the cdylib onto the same `native_modules/<stem>.so` convention.
    for stem in [
        "strings",
        "math",
        "http_client",
        "env",
        "terminal",
        "json",
        "crypto",
        "jwt",
        "markdown",
        "fs",
        "json_store",
        "process",
        "git",
        "mailer",
        "oauth",
        "mongodb",
        "blob_storage",
        "cloud_drive",
        "localai",
        "net_server",
        "net_client",
        "mailer_mock",
        "oauth_mock",
        "mongodb_mock",
        "blob_storage_mock",
        "cloud_drive_mock",
        "git_mock",
        "localai_mock",
        "test_math",
        "test_events",
        "test_timer",
        "test_panics",
    ] {
        // `test_panics` is a test double rather than a shipped module, so
        // it lives beside the other doubles instead of under crates/modules/.
        // The `test_*` doubles live beside the fixtures; the shipped
        // modules live under crates/modules/.
        let crate_dir = if stem.starts_with("test_") {
            modules_dir.join(stem)
        } else {
            manifest_dir.join("crates/modules").join(stem)
        };
        let cargo_status = Command::new("cargo")
            .args(["build", "--release"])
            .current_dir(&crate_dir)
            .status()
            .unwrap_or_else(|e| panic!("failed to run cargo for {stem}: {e}"));
        assert!(
            cargo_status.success(),
            "cargo failed to build crates/modules/{stem}"
        );
        let built = crate_dir.join(format!("target/release/lib{stem}.so"));
        let dest = modules_dir.join(format!("{stem}.so"));
        fs::rename(&built, &dest).unwrap_or_else(|e| {
            panic!("cannot move {} to {}: {e}", built.display(), dest.display())
        });
    }
}

/// Compiles `test_math_static.c` and `test_math_static_ambiguous.c` into the
/// `.a` archives the `buildonly_native_link_static_*`/
/// `fail_native_link_static_*` fixtures `link` — a `cargo build` of a
/// `staticlib` crate, which emits the archive directly, mirroring
/// `build_native_dynamic_test_modules`' `cargo build` for the `.so`
/// case.
fn build_native_static_test_modules(tests_dir: &Path) {
    let modules_dir = tests_dir.join("native_modules");
    for stem in [
        "test_math_static",
        "test_math_static_ambiguous",
        "test_events_static",
    ] {
        let crate_dir = modules_dir.join(stem);
        // `cargo` emits the archive itself — no `cc -c` and `ar rcs` here,
        // because `crate-type = ["staticlib"]` is exactly that pair. The
        // crates take `code-native`'s `static-module` feature, which is what
        // keeps a second copy of `runtime.c` out of the archive.
        let status = Command::new("cargo")
            .args(["build", "--release"])
            .current_dir(&crate_dir)
            .status()
            .unwrap_or_else(|e| panic!("failed to run cargo for {stem}: {e}"));
        assert!(status.success(), "cargo failed to build {stem}");
        let built = crate_dir.join(format!("target/release/lib{stem}.a"));
        let archive = modules_dir.join(format!("{stem}.a"));
        fs::copy(&built, &archive).unwrap_or_else(|e| {
            panic!(
                "cannot copy {} to {}: {e}",
                built.display(),
                archive.display()
            )
        });
    }
}

/// Runs the fixture through `code run` in a child process. See the top-of-file
/// note on why the interpret check is a subprocess rather than an in-process
/// `code::run_file` call.
fn check_interpret(name: &str, path: &Path, expect: Expect, failures: &mut Vec<String>) {
    // `BuildOnly` behaves like `Fail` here — a `.a` link is refused by
    // `interpreter.rs` outright, the same shape of error as any other
    // fail_*.code fixture, just for a different reason.
    let should_fail = expect != Expect::Succeed;
    let output = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("run")
        .arg(path)
        .output()
        .unwrap_or_else(|e| panic!("{name}: run `code run` subprocess: {e}"));

    match (should_fail, output.status.success()) {
        (false, false) => failures.push(format!(
            "{name}: interpret: expected to run, but exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )),
        (true, true) => failures.push(format!("{name}: interpret: expected an error, but it ran")),
        _ => {}
    }
}

fn check_compile(
    name: &str,
    path: &Path,
    expect: Expect,
    tmp_dir: &Path,
    failures: &mut Vec<String>,
) {
    let stem = name.trim_end_matches(".code");
    let exe_path: PathBuf = tmp_dir.join(stem);
    let should_fail = expect == Expect::Fail;

    // This harness checks behaviour, not container shape — every fixture
    // compiles to the default `Exe` target (`--target shared|static|wasm`
    // is covered separately in `tests/build_targets.rs`).
    match code::compile_file(path, code::BuildTarget::Exe, &exe_path, false) {
        Err(e) => {
            // A compile-time failure (parse error, undefined variable) is a
            // valid way for a fail_*.code fixture to fail — nothing further
            // to check either way.
            if !should_fail {
                failures.push(format!(
                    "{name}: compile: expected to compile, but errored: {e}"
                ));
            }
        }
        Ok(()) => {
            // Turns "every value the program allocated was released" into an
            // observable pass/fail: the runtime counts live heap blocks and,
            // with this set, aborts at exit if any survive codegen's cleanup
            // (see `code_check_leaks` in runtime.c). Without it a lost
            // reference would produce byte-identical output to a correct run.
            let output = Command::new(&exe_path)
                .env("CODE_CHECK_LEAKS", "1")
                .output()
                .unwrap_or_else(|e| panic!("{name}: run compiled binary: {e}"));

            if should_fail {
                if output.status.success() {
                    failures.push(format!(
                        "{name}: compile: expected an error (compile-time or at runtime), but the binary ran successfully"
                    ));
                }
                // A non-zero exit is exactly what a fail_*.code fixture
                // whose error only exists at runtime (a type mismatch,
                // division by zero) is expected to produce — nothing
                // further to check.
            } else if !output.status.success() {
                failures.push(format!(
                    "{name}: compile: expected the binary to run cleanly, but it exited with {}; stderr: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                ));
            } else if expect == Expect::BuildOnly {
                // No interpreted run to compare against — `code run` never
                // produces bindings for a `.a` link at all (see
                // `check_interpret`).
            }
            let _ = fs::remove_file(&exe_path);
        }
    }
}

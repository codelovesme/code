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
//!
//! `FIXTURES=console_,fail_emit` runs only the fixtures whose names start
//! with one of the comma-separated prefixes, and builds only the modules
//! those fixtures `link` — the quick loop while working on one module
//! (`scripts/test-changed.sh` sets it). Unset, everything runs, as on CI.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "support/modules.rs"]
mod modules;

#[test]
fn code_fixtures_run_as_expected() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let tmp_dir = std::env::temp_dir().join("code-compiler-tests");
    fs::create_dir_all(&tmp_dir).expect("create temp dir for compiled fixtures");

    let mut fixtures: Vec<(String, PathBuf, Expect)> = Vec::new();
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
        fixtures.push((name, path, expect));
    }
    assert!(
        !fixtures.is_empty(),
        "no .code fixtures found in {}",
        dir.display()
    );

    let filter = std::env::var("FIXTURES").ok().map(|prefixes| {
        prefixes
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    });
    // Everything the run needs, and — when filtered — nothing else: building
    // forty-odd modules to run three fixtures is the wait this skips.
    let wanted: Option<Vec<String>> = match &filter {
        None => None,
        Some(prefixes) => {
            fixtures.retain(|(name, _, _)| prefixes.iter().any(|p| name.starts_with(p.as_str())));
            assert!(
                !fixtures.is_empty(),
                "FIXTURES={} matches no fixture",
                prefixes.join(",")
            );
            Some(linked_modules(&fixtures))
        }
    };
    let wants = |stem: &str| wanted.as_ref().is_none_or(|w| w.iter().any(|m| m == stem));
    build_native_dynamic_test_modules(&dir, &wants);
    build_native_static_test_modules(&dir, &wants);
    build_code_test_guests(&dir);
    // Sorted so a failing run names its fixtures in the same order every
    // time; the threads below finish in whatever order they finish.
    fixtures.sort_by(|a, b| a.0.cmp(&b.0));

    // One LLVM compile and link per fixture is the whole cost here — ~0.2s
    // each, and there are nearly 300 of them, which is a minute and a half
    // on one core of twelve. Split it across the machine instead.
    //
    // Safe by construction, and already proven: each fixture compiles to its
    // own path under `tmp_dir`, and `tests/concurrent_builds.rs` exists for
    // exactly this case — `compile_file` used to write `code_abi.h` beside
    // the output under a fixed name and delete it after linking, so parallel
    // builds deleted the header out from under each other. `compile`'s
    // `scratch_dir` is what fixed that.
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(fixtures.len());
    let chunk = fixtures.len().div_ceil(threads);
    let tmp_dir = &tmp_dir;
    let mut failures: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = fixtures
            .chunks(chunk)
            .map(|batch| {
                scope.spawn(move || {
                    let mut mine = Vec::new();
                    for (name, path, expect) in batch {
                        // Both modes take the fixture's *path*, not its text:
                        // `link` resolves relative to the linking file, so
                        // `tests/modules/*.code` is only reachable from a
                        // caller that knows where the fixture is. Those
                        // module files live in a subdirectory and so are
                        // never picked up as fixtures in their own right.
                        check_interpret(name, path, *expect, &mut mine);
                        check_compile(name, path, *expect, tmp_dir, &mut mine);
                    }
                    mine
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("fixture thread panicked"))
            .collect()
    });
    failures.sort();

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every `native_modules/<stem>.so` or `.a` the given fixtures name — which
/// is every module they `link`, since that is the only way a fixture reaches
/// one.
fn linked_modules(fixtures: &[(String, PathBuf, Expect)]) -> Vec<String> {
    let mut stems = Vec::new();
    for (_, path, _) in fixtures {
        let text = fs::read_to_string(path).unwrap_or_default();
        for (at, _) in text.match_indices("native_modules/") {
            let rest = &text[at + "native_modules/".len()..];
            let stem: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !stem.is_empty() && !stems.contains(&stem) {
                stems.push(stem);
            }
        }
    }
    stems
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
/// `native_link_*`/`fail_native_link_*`, `console_*`, `strings_*`,
/// `dom_*`, `clipboard_*`, `guest_*`, `media_*`, `math_*`, `json_*`, `json_store*`, `crypto_*`, `jwt_*`, `markdown_*`, `fs_*`, `process_*`, `git_*`, `mailer_*`, `azure_mailer_*`, `oauth_*`, `mongodb_*`, `azure_blob_*`, `blob_storage_*`, `cloud_drive_*`, `localai_*`, `searchxng_*`, `api_registry_*`, `*_mock_*`, `net_*`, `tty_*`, `pty_*`, `syntax_*`, and `http_client_*` fixtures `link` — checked into git as source, not
/// as a binary
/// (see `.gitignore`), so it has to be built fresh here before any fixture
/// that needs it can run either mode. Sources live next to their consumers:
/// `test_math` is a pure test double (stays in `tests/native_modules/`),
/// while `console`, `strings`, `math`, `env`, `json`, and `http_client` are
/// real first-party modules
/// that happen to be exercised by fixtures (their canonical homes are under
/// `crates/modules/`, where the release CI builds them from). The C modules
/// go straight through `cc`; the rest are Rust-on-`code-native` modules, so
/// they get a `cargo build` instead — same output location, same stem, the
/// fixtures cannot tell the difference. `http_client` is much the slowest to
/// build cold (it pulls ureq and rustls); nothing here needs a network,
/// though — its fixtures only ever talk to a refused port on loopback.
fn build_native_dynamic_test_modules(tests_dir: &Path, wants: &dyn Fn(&str) -> bool) {
    let modules_dir = tests_dir.join("native_modules");
    // The Rust modules: `cargo build` inside each standalone workspace (into
    // the shared module target directory — see `tests/support/modules.rs`),
    // then copy the cdylib onto the same `native_modules/<stem>.so`
    // convention.
    for stem in [
        "strings",
        "math",
        "membrane",
        "http_client",
        "env",
        "console",
        "json",
        "searchxng",
        "api_registry",
        "crypto",
        "jwt",
        "markdown",
        "fs",
        "json_store",
        "process",
        "git",
        "azure_mailer",
        "mailer",
        "oauth",
        "mongodb",
        "azure_blob",
        "blob_storage",
        "cloud_drive",
        "localai",
        "interpreter",
        "net_server",
        "net_client",
        "dom",
        "clipboard",
        "guest",
        "timer",
        "mqtt",
        "ntfy",
        "pty",
        "tty",
        "syntax",
        "window",
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
        if !wants(stem) {
            continue;
        }
        let built = modules::build_so(stem);
        let dest = modules_dir.join(format!("{stem}.so"));
        // Copied, not moved: `tests/hosted_app.rs` builds the same crates
        // for itself and takes them from the shared target directory, and a
        // move would pull the file out from under it whenever the two suites
        // overlap.
        fs::copy(&built, &dest).unwrap_or_else(|e| {
            panic!("cannot copy {} to {}: {e}", built.display(), dest.display())
        });
    }
}

/// Builds the modules written in `code` that the `runtime_link_keeps_*`
/// fixtures link — `native_modules/test_keeper.code` to `test_keeper.so`,
/// the `--target shared` build any program gets when another will link it.
/// A fixture cannot build, so the runner does it here, beside the Rust
/// doubles.
fn build_code_test_guests(tests_dir: &Path) {
    let modules_dir = tests_dir.join("native_modules");
    // One today; a list so the next is a line.
    let guests = ["test_keeper"];
    for stem in guests {
        let source = modules_dir.join(format!("{stem}.code"));
        let dest = modules_dir.join(format!("{stem}.so"));
        code::compile_file(&source, code::BuildTarget::Shared, &dest, false)
            .unwrap_or_else(|e| panic!("build {stem}.code as a shared module: {e}"));
    }
}

/// Compiles `test_math_static.c` and `test_math_static_ambiguous.c` into the
/// `.a` archives the `buildonly_native_link_static_*`/
/// `fail_native_link_static_*` fixtures `link` — a `cargo build` of a
/// `staticlib` crate, which emits the archive directly, mirroring
/// `build_native_dynamic_test_modules`' `cargo build` for the `.so`
/// case.
fn build_native_static_test_modules(tests_dir: &Path, wants: &dyn Fn(&str) -> bool) {
    let modules_dir = tests_dir.join("native_modules");
    for stem in [
        "test_math_static",
        "test_math_static_ambiguous",
        "test_events_static",
    ] {
        if !wants(stem) {
            continue;
        }
        // `cargo` emits the archive itself — no `cc -c` and `ar rcs` here,
        // because `crate-type = ["staticlib"]` is exactly that pair. The
        // crates take `code-native`'s `static-module` feature, which is what
        // keeps a second copy of `runtime.c` out of the archive.
        let built = modules::build_a(stem);
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

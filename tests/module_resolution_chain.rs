//! End-to-end coverage for the module resolution fallback chain and the
//! load-time sha256 re-check (`docs/todo/community-modules.md`, phases C–D):
//!
//! 1. script-directory lookup still wins (unchanged behaviour);
//! 2. `<nearest .code>/modules/<name>/<version>/` is found without any env
//!    configuration — exactly where `code install` lays bytes down;
//! 3. `$CODE_MODULE_PATH` entries are consulted after the project dir;
//! 4. `~/.code/modules/` is the last resort;
//! 5. while a lock entry pins the resolved bytes, tampering with them makes
//!    loading fail loudly instead of silently executing different code.
//!
//! These drive the real interpreter in-process via `code::run_file`, so they
//! exercise the same `FilesystemResolver` the CLI uses. Each test owns its
//! own fake `$HOME` and builds its own copy of `test_math.so` (the checked-in
//! source, same recipe as `run_language_tests.rs`), so the four roots never
//! collide and no test depends on another test's artifacts. Environment
//! changes are restored afterwards.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

/// Serializes the tests' `HOME`/`CODE_MODULE_PATH` swaps: those are process
/// globals, and libtest runs the five tests on parallel threads, so without
/// this lock two tests can interleave their set/remove pairs and one sees
/// the other's fake root (an intermittent `resolves_from_*` failure).
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_guard() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

const MAIN_CODE: &str = r#"link "test_math.so" as m
emit Double { "value": 21 } to m get n
assert n = { "_class": "DoubleResult", "value": 42 }
"#;

fn fixture_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("code-resolver-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Compile `tests/native_modules/test_math.c` into `dest` — the same recipe
/// the fixture harness uses (`run_language_tests.rs`), kept local so this
/// suite does not depend on which other test happened to run first.
fn build_test_math(dest: &Path) {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let status = Command::new("cc")
        .args(["-shared", "-fPIC"])
        .arg("-o")
        .arg(dest)
        .arg(manifest_dir.join("tests/native_modules/test_math.c"))
        .args(["-lm", "-ldl"])
        .status()
        .unwrap_or_else(|e| panic!("failed to run cc for test_math: {e}"));
    assert!(status.success(), "cc failed to build test_math.so");
}

/// Run `main.code` from `script_dir` with `home` as `$HOME` and
/// `module_path` as `$CODE_MODULE_PATH` (either may be `None`). Returns
/// whether the interpreter succeeded.
fn run_with_env(script_dir: &Path, home: Option<&Path>, module_path: Option<&Path>) -> bool {
    let old_home = std::env::var_os("HOME");
    let old_cmp = std::env::var_os("CODE_MODULE_PATH");
    match home {
        Some(h) => std::env::set_var("HOME", h),
        None => std::env::remove_var("HOME"),
    }
    match module_path {
        Some(p) => std::env::set_var("CODE_MODULE_PATH", p),
        None => std::env::remove_var("CODE_MODULE_PATH"),
    }
    let entry = script_dir.join("main.code");
    let result = code::run_file(&entry);
    match old_home {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
    }
    match old_cmp {
        Some(v) => std::env::set_var("CODE_MODULE_PATH", v),
        None => std::env::remove_var("CODE_MODULE_PATH"),
    }
    result.is_ok()
}

#[test]
fn resolves_from_script_directory_first() {
    let _env = env_guard();
    let dir = fixture_dir("scriptdir");
    build_test_math(&dir.join("test_math.so"));
    fs::write(dir.join("main.code"), MAIN_CODE).unwrap();
    assert!(
        run_with_env(&dir, None, None),
        "script-dir lookup must keep working"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn resolves_from_project_modules_dir() {
    let _env = env_guard();
    let dir = fixture_dir("project");
    // main.code lives in a subdir; the module sits in <root>/.code/modules/,
    // i.e. NOT next to the script — only the fallback chain reaches it.
    let script = dir.join("src");
    fs::create_dir_all(&script).unwrap();
    fs::write(script.join("main.code"), MAIN_CODE).unwrap();
    let modules = dir.join(".code").join("modules");
    fs::create_dir_all(&modules).unwrap();
    build_test_math(&modules.join("test_math.so"));
    assert!(
        run_with_env(&script, None, None),
        ".code/modules must be reachable without env configuration"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn resolves_from_code_module_path() {
    let _env = env_guard();
    let dir = fixture_dir("envpath");
    let script = dir.join("proj");
    fs::create_dir_all(&script).unwrap();
    fs::write(script.join("main.code"), MAIN_CODE).unwrap();
    let extra = dir.join("somewhere-else");
    fs::create_dir_all(&extra).unwrap();
    build_test_math(&extra.join("test_math.so"));
    assert!(
        run_with_env(&script, None, Some(&extra)),
        "$CODE_MODULE_PATH entries must be searched"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn resolves_from_global_home_modules() {
    let _env = env_guard();
    let dir = fixture_dir("global");
    let script = dir.join("proj");
    fs::create_dir_all(&script).unwrap();
    fs::write(script.join("main.code"), MAIN_CODE).unwrap();
    let home = dir.join("fake-home");
    let modules = home.join(".code").join("modules");
    fs::create_dir_all(&modules).unwrap();
    build_test_math(&modules.join("test_math.so"));
    assert!(
        run_with_env(&script, Some(&home), None),
        "~/.code/modules must be the final fallback"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn refuses_tampered_module_while_locked() {
    let _env = env_guard();
    let dir = fixture_dir("tamper");
    let script = dir.join("proj");
    fs::create_dir_all(&script).unwrap();
    fs::write(script.join("main.code"), MAIN_CODE).unwrap();
    // The *installed* layout — exactly where `code install` puts bytes:
    // <root>/<name>/<version>/<asset>. Resolution of a bare asset name
    // depends on the lockfile below, which is the point being tested.
    let installed = dir
        .join(".code")
        .join("modules")
        .join("test_math")
        .join("0.0.0");
    fs::create_dir_all(&installed).unwrap();
    let so = installed.join("test_math.so");
    build_test_math(&so);

    // Pin the pristine bytes in the lockfile, exactly as `code install`
    // would.
    let digest = code::module_install::sha256_of(&so).unwrap();
    let lock_path = dir.join(".code").join("lock.json");
    let lock = serde_json::json!({
        "modules": {
            "test_math": {
                "name": "test_math",
                "version": "0.0.0",
                "source": "https://example.invalid/test_math",
                "asset": "test_math.so",
                "sha256": digest,
                "global": false
            }
        }
    });
    fs::write(&lock_path, serde_json::to_string_pretty(&lock).unwrap()).unwrap();

    // Pristine bytes load fine…
    assert!(
        run_with_env(&script, None, None),
        "pristine locked module loads"
    );

    // …but a byte-flip in place is refused rather than executed.
    let mut bytes = fs::read(&so).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    fs::write(&so, bytes).unwrap();
    assert!(
        !run_with_env(&script, None, None),
        "tampered bytes behind a lock entry must refuse to load"
    );

    // Without the lock entry the installed layout is invisible — the
    // lockfile is what maps a bare asset name to its pinned location.
    fs::remove_file(&lock_path).unwrap();
    assert!(
        !run_with_env(&script, None, None),
        "an installed-layout module with no lock entry cannot be resolved"
    );
    let _ = fs::remove_dir_all(&dir);
}

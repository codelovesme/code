//! `code run` builds the program first and keeps the build (ticket 124).
//!
//! The rest of the tests run with `CODE_RUN=interpret` (`.cargo/config.toml`),
//! so that `code run` stays the interpreter they compare compiled code with;
//! these clear it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("code-run-built-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(dir: &Path, cache: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_code"))
        .args(args)
        .current_dir(dir)
        .env_remove("CODE_RUN")
        .env("CODE_CACHE_DIR", cache)
        .output()
        .unwrap()
}

fn cached_runs(cache: &Path) -> usize {
    fs::read_dir(cache.join("run")).map_or(0, |d| {
        d.flatten()
            .filter(|e| e.path().extension().is_none())
            .count()
    })
}

#[test]
fn the_tests_interpret() {
    assert_eq!(std::env::var("CODE_RUN").as_deref(), Ok("interpret"));
}

#[test]
fn a_run_is_built_once_and_kept() {
    let dir = temp_dir("kept");
    let cache = dir.join("cache");
    fs::write(dir.join("p.code"), "x = 2\nassert x * 3 = 6\n").unwrap();
    assert!(run(&dir, &cache, &["run", "p.code"]).status.success());
    assert_eq!(cached_runs(&cache), 1);
    assert!(run(&dir, &cache, &["run", "p.code"]).status.success());
    assert_eq!(cached_runs(&cache), 1, "the second run reuses the build");
    fs::write(dir.join("p.code"), "x = 3\nassert x * 3 = 9\n").unwrap();
    assert!(run(&dir, &cache, &["run", "p.code"]).status.success());
    assert_eq!(cached_runs(&cache), 2, "a changed program is built again");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_failing_program_fails_the_same_way_built_or_interpreted() {
    let dir = temp_dir("fails");
    let cache = dir.join("cache");
    fs::write(dir.join("p.code"), "x = 2\nassert x = 3\n").unwrap();
    let built = run(&dir, &cache, &["run", "p.code"]);
    let interpreted = run(&dir, &cache, &["run", "--interpret", "p.code"]);
    assert!(!built.status.success());
    assert_eq!(built.status.code(), interpreted.status.code());
    assert_eq!(
        String::from_utf8_lossy(&built.stderr),
        String::from_utf8_lossy(&interpreted.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_program_the_compiler_refuses_is_reported_as_the_interpreter_reports_it() {
    let dir = temp_dir("refused");
    let cache = dir.join("cache");
    fs::write(dir.join("p.code"), "x = 1\nx = \"one\"\n").unwrap();
    let built = run(&dir, &cache, &["run", "p.code"]);
    let interpreted = run(&dir, &cache, &["run", "--interpret", "p.code"]);
    assert!(!built.status.success());
    assert_eq!(built.stderr, interpreted.stderr);
    assert_eq!(
        cached_runs(&cache),
        0,
        "nothing is kept for a refused program"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_large_program_starts_on_a_quick_build_and_gets_an_optimised_one() {
    let dir = temp_dir("large");
    let cache = dir.join("cache");
    let mut source = String::from("total = 0\n");
    let mut sum = 0;
    for i in 0..900 {
        source.push_str(&format!(
            "value_{i} = {i} * 2 + 1\ntotal = total + value_{i}\n"
        ));
        sum += i * 2 + 1;
    }
    source.push_str(&format!("assert total = {sum}\n"));
    fs::write(dir.join("p.code"), source).unwrap();
    assert!(run(&dir, &cache, &["run", "p.code"]).status.success());
    // The quick build at once; the optimised one from the background build.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while cached_runs(&cache) < 2 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(
        cached_runs(&cache),
        2,
        "the optimised build joins the quick one"
    );
    assert!(run(&dir, &cache, &["run", "p.code"]).status.success());
    let _ = fs::remove_dir_all(&dir);
}

fn record(cache: &Path) -> PathBuf {
    fs::read_dir(cache.join("run"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|p| p.extension().is_some_and(|e| e == "inputs"))
        .unwrap()
}

#[test]
fn an_edit_to_a_linked_source_never_runs_the_old_build() {
    let dir = temp_dir("linked-edit");
    let cache = dir.join("cache");
    fs::write(
        dir.join("p.code"),
        "link \"child.code\"\nemit Answer to this get r\nassert r.value = 1\n",
    )
    .unwrap();
    fs::write(
        dir.join("child.code"),
        "Answer {} => return R { value = 1 }\n",
    )
    .unwrap();
    assert!(run(&dir, &cache, &["run", "p.code"]).status.success());
    fs::write(
        dir.join("child.code"),
        "Answer {} => return R { value = 2 }\n",
    )
    .unwrap();
    let changed = run(&dir, &cache, &["run", "p.code"]);
    assert!(!changed.status.success(), "a stale build would still pass");
    assert!(String::from_utf8_lossy(&changed.stderr).contains("assertion failed"));
    assert_eq!(cached_runs(&cache), 2);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_reinstalled_static_module_at_the_same_path_is_rebuilt() {
    let dir = temp_dir("module-reinstalled");
    let cache = dir.join("cache");
    fs::write(
        dir.join("p.code"),
        "link \"module.a\" as m\nemit Answer to m get r\nassert r.value = 1\n",
    )
    .unwrap();
    for value in [1, 2] {
        fs::write(
            dir.join("module.code"),
            format!("Answer {{}} => return R {{ value = {value} }}\n"),
        )
        .unwrap();
        let build = run(
            &dir,
            &cache,
            &[
                "build",
                "module.code",
                "--target",
                "static",
                "-o",
                "module.a",
            ],
        );
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let result = run(&dir, &cache, &["run", "p.code"]);
        assert_eq!(
            result.status.success(),
            value == 1,
            "a stale static archive would still return 1"
        );
    }
    assert_eq!(cached_runs(&cache), 2);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_changed_or_new_lock_is_a_cache_miss_even_when_the_ast_is_the_same() {
    let dir = temp_dir("lock-changed");
    let cache = dir.join("cache");
    fs::write(dir.join("p.code"), "assert true\n").unwrap();
    assert!(run(&dir, &cache, &["run", "p.code"]).status.success());
    fs::create_dir(dir.join(".code")).unwrap();
    fs::write(dir.join(".code/lock.json"), "{}\n").unwrap();
    assert!(run(&dir, &cache, &["run", "p.code"]).status.success());
    assert_eq!(
        cached_runs(&cache),
        2,
        "an absent lock becoming present changes the key"
    );
    fs::write(dir.join(".code/lock.json"), "{\"modules\": {}}\n").unwrap();
    assert!(run(&dir, &cache, &["run", "p.code"]).status.success());
    assert_eq!(
        cached_runs(&cache),
        3,
        "the lock participates in the full key too"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_record_from_another_compiler_version_takes_the_full_path() {
    let dir = temp_dir("compiler-version");
    let cache = dir.join("cache");
    fs::write(dir.join("p.code"), "assert true\n").unwrap();
    assert!(run(&dir, &cache, &["run", "p.code"]).status.success());
    let path = record(&cache);
    let saved = fs::read_to_string(&path).unwrap().replace(
        &format!("version\t{}", env!("CARGO_PKG_VERSION")),
        "version\tanother compiler version",
    );
    fs::write(&path, saved).unwrap();
    assert!(run(&dir, &cache, &["run", "p.code"]).status.success());
    let renewed = fs::read_to_string(path).unwrap();
    assert!(
        renewed.contains(&format!("version\t{}\n", env!("CARGO_PKG_VERSION"))),
        "the full path replaces the stale version record"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_missing_or_corrupt_record_takes_the_full_path() {
    let dir = temp_dir("bad-record");
    let cache = dir.join("cache");
    fs::write(dir.join("p.code"), "assert true\n").unwrap();
    assert!(run(&dir, &cache, &["run", "p.code"]).status.success());
    let path = record(&cache);
    fs::write(&path, b"not a record").unwrap();
    assert!(run(&dir, &cache, &["run", "p.code"]).status.success());
    fs::remove_file(&path).unwrap();
    assert!(run(&dir, &cache, &["run", "p.code"]).status.success());
    assert!(path.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
fn replace_cached_program(cache: &Path, source: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let exe = fs::read_dir(cache.join("run"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|p| p.extension().is_none())
        .unwrap();
    fs::write(&exe, source).unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
    exe
}

#[cfg(unix)]
#[test]
fn unix_exec_keeps_the_pid_environment_streams_arguments_and_exit_status() {
    use std::io::Write;
    use std::process::Stdio;
    let dir = temp_dir("exec-process");
    let cache = dir.join("cache");
    fs::write(dir.join("p.code"), "assert true\n").unwrap();
    assert!(run(&dir, &cache, &["run", "p.code"]).status.success());
    // An observable stand-in for the artifact, after the real build established
    // a valid warm record. The old spawn-and-wait path would give it another PID.
    replace_cached_program(&cache, "#!/bin/sh\nread -r line\nprintf '%s:%s:%s:%s\\n' \"$$\" \"$#\" \"$CODE_EXEC_TEST\" \"$line\"\nprintf 'stderr intact\\n' >&2\nexit 7\n");
    let mut child = Command::new(env!("CARGO_BIN_EXE_code"))
        .args(["run", "p.code"])
        .current_dir(&dir)
        .env_remove("CODE_RUN")
        .env("CODE_CACHE_DIR", &cache)
        .env("CODE_EXEC_TEST", "inherited")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"from stdin\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{pid}:0:inherited:from stdin\n")
    );
    assert_eq!(output.stderr, b"stderr intact\n");
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn unix_exec_preserves_signals() {
    use std::os::unix::process::ExitStatusExt;
    let dir = temp_dir("exec-signals");
    let cache = dir.join("cache");
    fs::write(dir.join("p.code"), "assert true\n").unwrap();
    assert!(run(&dir, &cache, &["run", "p.code"]).status.success());
    replace_cached_program(&cache, "#!/bin/sh\nkill -TERM $$\n");
    assert_eq!(
        run(&dir, &cache, &["run", "p.code"]).status.signal(),
        Some(libc::SIGTERM)
    );
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn a_failed_exec_interprets_with_the_same_note() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir("exec-failed");
    let cache = dir.join("cache");
    fs::write(dir.join("p.code"), "assert true\n").unwrap();
    assert!(run(&dir, &cache, &["run", "p.code"]).status.success());
    let exe = replace_cached_program(&cache, "not executable\n");
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o644)).unwrap();
    let output = run(&dir, &cache, &["run", "p.code"]);
    assert!(output.status.success());
    let note = String::from_utf8(output.stderr).unwrap();
    assert!(note.starts_with("note: could not start the built program ("));
    assert!(note.ends_with("); interpreting it\n"));
    fs::remove_dir_all(dir).unwrap();
}

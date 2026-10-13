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
    fs::read_dir(cache.join("run")).map_or(0, |d| d.flatten().count())
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

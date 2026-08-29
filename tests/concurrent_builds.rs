//! `code build` has to be safe to run concurrently, including several builds
//! sharing one output directory — a `make -j`, or a test harness compiling
//! fixtures in parallel.
//!
//! It was not: `compile_file` wrote `code_abi.h` beside `exe_path` under
//! exactly that name (`runtime.c` `#include`s it, so it cannot be renamed)
//! and deleted it after linking, so whichever build finished first deleted
//! the header the others were still compiling against. Reproduced at 9
//! failures out of 30 concurrent builds before the fix; see `compile`'s
//! `scratch_dir` in `src/lib.rs`.

#![cfg(feature = "llvm")]

use std::fs;
use std::path::PathBuf;
use std::thread;

/// Enough statements that `cc` spends real time on each program: with
/// trivial ones the window between writing the header and deleting it is too
/// short for the race to land, and this test passes even against the bug.
fn program(seed: usize) -> String {
    let mut src = format!("let a = {seed}\n");
    for k in 0..120 {
        src.push_str(&format!("let v{k} = [{k}, \"s{k}\", {{ f = {k} }}]\n"));
    }
    src.push_str(&format!("assert a = {seed}\n"));
    src
}

#[test]
fn concurrent_builds_sharing_an_output_directory_all_succeed() {
    const N: usize = 12;
    let dir = std::env::temp_dir().join(format!("code-concurrent-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create test directory");

    let sources: Vec<PathBuf> = (0..N)
        .map(|i| {
            let path = dir.join(format!("p{i}.code"));
            fs::write(&path, program(i)).expect("write fixture");
            path
        })
        .collect();

    let failures: Vec<String> = thread::scope(|scope| {
        let handles: Vec<_> = sources
            .iter()
            .enumerate()
            .map(|(i, source)| {
                let exe = dir.join(format!("out{i}"));
                scope.spawn(move || {
                    match code::compile_file(source, code::BuildTarget::Exe, &exe, false) {
                        Ok(()) => None,
                        Err(e) => Some(format!("build {i}: {e}")),
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().expect("build thread panicked"))
            .collect()
    });

    let _ = fs::remove_dir_all(&dir);
    assert!(
        failures.is_empty(),
        "{} of {N} concurrent builds failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The intermediates belong in the build's own scratch directory, not beside
/// the binary — a build should leave exactly the executable it was asked for.
#[test]
fn a_build_leaves_no_intermediates_beside_its_output() {
    let dir = std::env::temp_dir().join(format!("code-artifacts-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create test directory");
    let source = dir.join("prog.code");
    fs::write(&source, "let a = 1\nassert a = 1\n").expect("write fixture");

    let exe = dir.join("prog");
    code::compile_file(&source, code::BuildTarget::Exe, &exe, false).expect("build");

    let mut left: Vec<String> = fs::read_dir(&dir)
        .expect("read test directory")
        .map(|e| {
            e.expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    left.sort();
    let _ = fs::remove_dir_all(&dir);

    assert_eq!(
        left,
        vec!["prog".to_string(), "prog.code".to_string()],
        "expected only the source and the executable"
    );
}

//! A scope error should identify a linked source file owning the name.
#![cfg(feature = "llvm")]
use std::{fs, path::Path};

fn check_both(entry: &Path, owner: &str) {
    let interpreted = code::run_file(entry).expect_err("the binding must remain inaccessible");
    let compiled = code::compile_file(
        entry,
        code::BuildTarget::Exe,
        &entry.with_extension("exe"),
        false,
    )
    .expect_err("compilation must preserve module isolation");
    assert_eq!(interpreted, compiled);
    assert!(
        compiled.contains("undefined variable 'count'"),
        "{compiled}"
    );
    assert!(
        compiled.contains(owner),
        "the declaring file must be identified: {compiled}"
    );
}

#[test]
fn inaccessible_top_level_bindings_name_their_owning_file_without_changing_scope() {
    let dir = std::env::temp_dir().join(format!("code-linked-binding-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("state.code"), "count = 1\n").unwrap();
    fs::write(
        dir.join("view.code"),
        "Read => return Value { value = count }\n",
    )
    .unwrap();
    let main = dir.join("main.code");
    fs::write(&main, "link \"state.code\"\nlink \"view.code\"\n").unwrap();
    check_both(&main, "state.code");

    // A child cannot read its caller's binding either.
    fs::write(&main, "count = 1\nlink \"view.code\"\n").unwrap();
    check_both(&main, "main.code");

    // A file merely sitting next to the entry is outside its program.
    fs::write(&main, "link \"view.code\"\n").unwrap();
    let err = code::run_file(&main).unwrap_err();
    assert!(
        !err.contains("state.code"),
        "unlinked files must not leak: {err}"
    );

    // Handler locals are not top-level state and must not be suggested.
    fs::write(&main, "Other =>\n    count = 1\nlink \"view.code\"\n").unwrap();
    let err = code::run_file(&main).unwrap_err();
    assert!(
        !err.contains("declared in"),
        "a different handler's locals are irrelevant: {err}"
    );
    fs::remove_dir_all(dir).unwrap();
}

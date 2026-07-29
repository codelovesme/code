//! Verifies the CLI renders rustc-style parse diagnostics.

use std::fs;
use std::process::Command;

#[test]
fn parse_error_is_rendered_with_caret() {
    let exe = env!("CARGO_BIN_EXE_code");
    let path = std::env::temp_dir().join(format!("code_diag_{}.code", std::process::id()));
    fs::write(&path, "x = 1\ny = @\nz = 2\n").unwrap();

    let out = Command::new(exe)
        .args(["run", path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(!out.status.success(), "a syntax error should fail the run");

    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("error:"), "missing `error:` header:\n{text}");
    assert!(text.contains("--> "), "missing source location arrow:\n{text}");
    assert!(text.contains(":2:"), "should point at line 2:\n{text}");
    assert!(text.contains('^'), "missing caret underline:\n{text}");
    assert!(text.contains("y = @"), "should echo the offending source line:\n{text}");

    let _ = fs::remove_file(&path);
}

#[test]
fn runtime_type_error_names_the_found_type() {
    let exe = env!("CARGO_BIN_EXE_code");
    let path = std::env::temp_dir().join(format!("code_rt_{}.code", std::process::id()));
    // `if` on a Number is a runtime type error.
    fs::write(&path, "if 5 {\n  x = 1\n}\n").unwrap();

    let out = Command::new(exe)
        .args(["run", path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(
        text.contains("must be a Boolean") && text.contains("found Number"),
        "runtime type error should name the found type:\n{text}"
    );
    // Single-file runtime errors are located (file:line:col + caret).
    assert!(text.contains("--> "), "should carry a source location:\n{text}");
    assert!(text.contains(":1:"), "the `if` statement is on line 1:\n{text}");
    assert!(text.contains('^'), "should carry a caret:\n{text}");

    let _ = fs::remove_file(&path);
}

#[test]
fn runtime_error_in_linked_program_falls_back_to_plain() {
    let exe = env!("CARGO_BIN_EXE_code");
    let dir = std::env::temp_dir().join(format!("code_link_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("lib.code"), "greeting = \"hi\"\n").unwrap();
    fs::write(dir.join("main.code"), "link lib\nr = not 5\n").unwrap();

    let out = Command::new(exe)
        .args(["run", "main.code"])
        .current_dir(&dir)
        .output()
        .unwrap();

    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stderr);
    // Multi-file programs fall back to a plain message (no possibly-wrong caret).
    assert!(text.contains("Runtime error:"), "expected a plain message:\n{text}");
    assert!(!text.contains("--> "), "must not render a source location:\n{text}");

    let _ = fs::remove_dir_all(&dir);
}

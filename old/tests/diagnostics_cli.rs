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
fn codegen_error_is_located_in_single_file() {
    let exe = env!("CARGO_BIN_EXE_code");
    let path = std::env::temp_dir().join(format!("code_cg_{}.code", std::process::id()));
    // `break` outside a loop is rejected by the LLVM backend.
    fs::write(&path, "a = 1\nb = 2\nbreak\n").unwrap();

    let out = Command::new(exe)
        .args(["build", path.to_str().unwrap(), "--target", "ir"])
        .output()
        .unwrap();

    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("outside of a loop"), "expected the codegen message:\n{text}");
    assert!(text.contains("--> "), "codegen error should be located:\n{text}");
    assert!(text.contains(":3:"), "break is on line 3:\n{text}");
    assert!(text.contains('^'), "should carry a caret:\n{text}");

    let _ = fs::remove_file(&path);
}

#[test]
fn linked_program_error_in_main_is_located_to_main() {
    let exe = env!("CARGO_BIN_EXE_code");
    let dir = std::env::temp_dir().join(format!("code_link_main_{}", std::process::id()));
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
    assert!(text.contains("--> "), "linked programs are now located:\n{text}");
    assert!(text.contains("main.code:2:"), "error is on line 2 of main.code:\n{text}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn linked_program_error_in_module_points_to_that_module() {
    let exe = env!("CARGO_BIN_EXE_code");
    let dir = std::env::temp_dir().join(format!("code_link_mod_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    // The error lives in the imported module and fires while it is linked in.
    fs::write(dir.join("lib.code"), "greeting = \"hi\"\nbad = not 5\n").unwrap();
    fs::write(dir.join("main.code"), "link lib\na = 1\n").unwrap();

    let out = Command::new(exe)
        .args(["run", "main.code"])
        .current_dir(&dir)
        .output()
        .unwrap();

    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stderr);
    // Provenance resolves to the module's own file, not main.code.
    assert!(text.contains("lib.code:2:"), "should point into lib.code:\n{text}");
    assert!(!text.contains("main.code"), "should not blame main.code:\n{text}");

    let _ = fs::remove_dir_all(&dir);
}

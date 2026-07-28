use std::fs;
use std::process::Command;

#[test]
fn build_emits_ir_file() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["build", "tests/basic_assignment.code", "--target", "ir"])
        .status()
        .expect("failed to run code build");
    assert!(status.success());

    let ir = fs::read_to_string("target/llvm/basic_assignment.ll")
        .expect("missing IR output");
    assert!(ir.contains("define i32 @main"));
}

#[test]
fn build_rejects_shadowing() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["build", "tests/fail_block_shadow.code"])
        .status()
        .expect("failed to run code build");
    assert!(!status.success());
}

#[test]
fn build_native_executable() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["build", "tests/basic_assignment.code", "--target", "exe"])
        .status()
        .expect("failed to run code build");
    assert!(status.success());

    let out_path = "target/llvm/basic_assignment";
    assert!(
        std::path::Path::new(out_path).exists(),
        "executable not created"
    );

    let run_status = Command::new(out_path)
        .status()
        .expect("failed to run compiled executable");
    assert!(run_status.success(), "compiled executable returned error");
}

#[test]
fn build_shared_library() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["build", "tests/basic_assignment.code", "--target", "shared"])
        .status()
        .expect("failed to run code build");
    assert!(status.success());

    let out_path = "target/llvm/libbasic_assignment.so";
    assert!(std::path::Path::new(out_path).exists(), ".so not created");

    let output = Command::new("file")
        .arg(out_path)
        .output()
        .expect("failed to run file");
    let desc = String::from_utf8_lossy(&output.stdout);
    assert!(desc.contains("shared object"), "not a shared object: {}", desc);
}

#[test]
fn build_static_library() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["build", "tests/basic_assignment.code", "--target", "static"])
        .status()
        .expect("failed to run code build");
    assert!(status.success());

    let out_path = "target/llvm/libbasic_assignment.a";
    assert!(std::path::Path::new(out_path).exists(), ".a not created");

    let output = Command::new("file")
        .arg(out_path)
        .output()
        .expect("failed to run file");
    let desc = String::from_utf8_lossy(&output.stdout);
    assert!(desc.contains("ar archive"), "not an ar archive: {}", desc);
}

#[test]
fn build_wasm_module() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["build", "tests/basic_assignment.code", "--target", "wasm"])
        .status()
        .expect("failed to run code build");
    assert!(status.success());

    let out_path = "target/llvm/basic_assignment.wasm";
    assert!(std::path::Path::new(out_path).exists(), ".wasm not created");

    let output = Command::new("file")
        .arg(out_path)
        .output()
        .expect("failed to run file");
    let desc = String::from_utf8_lossy(&output.stdout);
    assert!(desc.contains("WebAssembly"), "not a WASM module: {}", desc);
}

#[test]
fn build_default_is_exe() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["build", "tests/equal_numbers.code"])
        .status()
        .expect("failed to run code build");
    assert!(status.success());

    // The default target is a native executable (no extension).
    let out_path = "target/llvm/equal_numbers";
    assert!(
        std::path::Path::new(out_path).exists(),
        "default build should produce an executable"
    );
    let run_status = Command::new(out_path)
        .status()
        .expect("failed to run compiled executable");
    assert!(run_status.success(), "default-built executable failed");
}

#[test]
fn build_object_ir() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["build", "tests/object_basic.code", "--target", "ir"])
        .status()
        .expect("failed to run code build");
    assert!(status.success());

    let ir = fs::read_to_string("target/llvm/object_basic.ll")
        .expect("missing IR output");
    assert!(ir.contains("@malloc"), "IR should call malloc for object allocation");
}

#[test]
fn build_object_exe_runs() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["build", "tests/object_basic.code", "--target", "exe"])
        .status()
        .expect("failed to run code build");
    assert!(status.success());

    let out_path = "target/llvm/object_basic";
    assert!(std::path::Path::new(out_path).exists(), "executable not created");

    let run_status = Command::new(out_path)
        .status()
        .expect("failed to run compiled executable");
    assert!(run_status.success(), "object_basic executable failed");
}

#[test]
fn build_object_equality_exe_runs() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["build", "tests/object_equality.code", "--target", "exe"])
        .status()
        .expect("failed to run code build");
    assert!(status.success());

    let out_path = "target/llvm/object_equality";
    assert!(std::path::Path::new(out_path).exists(), "executable not created");

    let run_status = Command::new(out_path)
        .status()
        .expect("failed to run compiled executable");
    assert!(run_status.success(), "object_equality executable failed");
}

#[test]
fn build_object_nested_exe_runs() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["build", "tests/object_nested.code", "--target", "exe"])
        .status()
        .expect("failed to run code build");
    assert!(status.success());

    let out_path = "target/llvm/object_nested";
    assert!(std::path::Path::new(out_path).exists(), "executable not created");

    let run_status = Command::new(out_path)
        .status()
        .expect("failed to run compiled executable");
    assert!(run_status.success(), "object_nested executable failed");
}

#[test]
fn build_native_import_alias_exe_runs() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["build", "tests/native_link_alias.code", "--target", "exe"])
        .status()
        .expect("failed to run code build");
    assert!(status.success(), "build with native import alias failed");

    let out_path = "target/llvm/native_link_alias";
    assert!(
        std::path::Path::new(out_path).exists(),
        "native_link_alias executable not created"
    );

    let run_status = Command::new(out_path)
        .status()
        .expect("failed to run compiled executable");
    assert!(
        run_status.success(),
        "native_link_alias executable failed"
    );
}

#[test]
fn build_native_import_flatten_exe_runs() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["build", "tests/native_link_flatten.code", "--target", "exe"])
        .status()
        .expect("failed to run code build");
    assert!(status.success(), "build with native import flatten failed");

    let out_path = "target/llvm/native_link_flatten";
    assert!(
        std::path::Path::new(out_path).exists(),
        "native_link_flatten executable not created"
    );

    let run_status = Command::new(out_path)
        .status()
        .expect("failed to run compiled executable");
    assert!(
        run_status.success(),
        "native_link_flatten executable failed"
    );
}

#[test]
fn build_native_import_wasm_rejected() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["build", "tests/native_link_alias.code", "--target", "wasm"])
        .status()
        .expect("failed to run code build");
    assert!(!status.success(), "wasm build with native import should fail");
}

#[test]
fn build_native_import_ir_succeeds() {
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["build", "tests/native_link_alias.code", "--target", "ir"])
        .status()
        .expect("failed to run code build");
    assert!(status.success(), "IR build with native import failed");

    let ir = fs::read_to_string("target/llvm/native_link_alias.ll")
        .expect("missing native_link_alias IR output");
    assert!(ir.contains("@__native_bridge_open"), "IR should contain native bridge calls");
}

#[test]
fn wasm_module_interpreter_runs() {
    // Test that the interpreter can load a .wasm module and execute against it.
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["run", "tests/native_link_wasm.code"])
        .status()
        .expect("failed to run code interpreter with wasm module");
    assert!(status.success(), "interpreter failed to execute native_link_wasm.code");
}

#[test]
fn wasm_module_so_on_wasm_target_rejected() {
    // .so imports should be rejected when --target wasm is used.
    let exe = env!("CARGO_BIN_EXE_code");
    let status = Command::new(exe)
        .args(["build", "tests/native_link_alias.code", "--target", "wasm"])
        .status()
        .expect("failed to run code build");
    assert!(!status.success(), "wasm build with .so native import should fail");
}

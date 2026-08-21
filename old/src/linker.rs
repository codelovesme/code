use std::path::Path;
use std::process::Command;

/// Link an object file into a native ELF executable.
/// `extra_objects` contains additional .o files (e.g. the native runtime).
/// `extra_flags` contains additional linker flags (e.g. "-ldl").
pub fn link_executable(
    object_path: &Path,
    output_path: &Path,
    extra_objects: &[&Path],
    extra_flags: &[&str],
) -> Result<(), String> {
    run_linker(&["cc", "-o"], output_path, object_path, extra_objects, extra_flags)
}

/// Link an object file into a shared library (.so).
pub fn link_shared(
    object_path: &Path,
    output_path: &Path,
    extra_objects: &[&Path],
    extra_flags: &[&str],
) -> Result<(), String> {
    run_linker(&["cc", "-shared", "-o"], output_path, object_path, extra_objects, extra_flags)
}

/// Pack an object file into a static library (.a).
pub fn link_static(object_path: &Path, output_path: &Path) -> Result<(), String> {
    let status = Command::new("ar")
        .args(["rcs"])
        .arg(output_path)
        .arg(object_path)
        .status()
        .map_err(|e| format!("Failed to run ar: {}", e))?;

    if !status.success() {
        return Err(format!("ar failed with exit code {:?}", status.code()));
    }
    Ok(())
}

/// Link a WASM object file into a standalone .wasm module.
pub fn link_wasm(object_path: &Path, output_path: &Path) -> Result<(), String> {
    // Try wasm-ld first, then fall back to wasm-ld-17
    let ld = find_wasm_ld()?;
    let status = Command::new(&ld)
        .args([
            "--no-entry",
            "--export-all",
            "--export=__stack_pointer",
            "--allow-undefined",
            "-o",
        ])
        .arg(output_path)
        .arg(object_path)
        .status()
        .map_err(|e| format!("Failed to run {}: {}", ld, e))?;

    if !status.success() {
        return Err(format!("{} failed with exit code {:?}", ld, status.code()));
    }
    Ok(())
}

fn find_wasm_ld() -> Result<String, String> {
    for name in &["wasm-ld", "wasm-ld-17", "wasm-ld-18", "wasm-ld-19"] {
        if Command::new(name)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Ok(name.to_string());
        }
    }
    Err("wasm-ld not found. Install lld (e.g. sudo apt install lld-17)".to_string())
}

fn run_linker(
    cmd_prefix: &[&str],
    output_path: &Path,
    object_path: &Path,
    extra_objects: &[&Path],
    extra_flags: &[&str],
) -> Result<(), String> {
    let (program, initial_args) = cmd_prefix
        .split_first()
        .ok_or_else(|| "Empty linker command".to_string())?;

    let output = Command::new(program)
        .args(initial_args)
        .arg(output_path)
        .arg(object_path)
        .args(extra_objects)
        .args(extra_flags)
        .output()
        .map_err(|e| format!("Failed to run {}: {}", program, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{} failed: {}", program, stderr.trim()));
    }
    Ok(())
}

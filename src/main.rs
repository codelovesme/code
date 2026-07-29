use code_lang::ast;
use code_lang::codegen;
use code_lang::interpreter::Interpreter;
use code_lang::linker;
use code_lang::module_loader;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

const VERSION: &str = "Code v0.1";

fn main() {
    // The parser combinator tree requires more stack space than the default.
    let builder = std::thread::Builder::new().stack_size(16 * 1024 * 1024);
    let handler = builder.spawn(run).expect("failed to spawn main thread");
    let code = handler.join().expect("main thread panicked");
    process::exit(code);
}

fn run() -> i32 {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return 0;
    }

    match args[1].as_str() {
        "build" => {
            if args.len() < 3 {
                eprintln!("Error: missing file argument");
                eprintln!("Usage: code build <file.code> [--target exe|ir|shared|static|wasm] [--release]");
                return 1;
            }
            let target = parse_target_flag(&args);
            let release = args.iter().any(|a| a == "--release");
            build_file(&args[2], &target, release)
        }
        "run" => {
            if args.len() < 3 {
                eprintln!("Error: missing file argument");
                eprintln!("Usage: code run <file.code>");
                return 1;
            }
            run_file(&args[2])
        }
        "test" => {
            run_tests()
        }
        "fmt" => {
            if args.len() < 3 {
                eprintln!("Error: missing file argument");
                eprintln!("Usage: code fmt <file.code> [--check]");
                return 1;
            }
            let check = args.iter().any(|a| a == "--check");
            fmt_file(&args[2], check)
        }
        "--version" | "-v" => {
            println!("{}", VERSION);
            0
        }
        other => {
            eprintln!("Unknown command: {}", other);
            print_usage();
            1
        }
    }
}

fn print_usage() {
    println!("{}", VERSION);
    println!();
    println!("Usage:");
    println!("  code build <file.code> [--target <type>] [--release]    Compile a .code file");
    println!("  code run <file.code>                        Interpret a .code file");
    println!("  code fmt <file.code> [--check]              Format a .code file in place");
    println!("  code test                                   Run all tests in tests/");
    println!("  code --version                              Print version");
    println!();
    println!("Build targets:");
    println!("  exe      Native executable              [default]");
    println!("  ir       LLVM IR text (.ll)");
    println!("  shared   Shared library (.so)");
    println!("  static   Static library (.a)");
    println!("  wasm     WebAssembly module (.wasm)");
}

/// Format a `.code` file in place, or verify formatting with `--check`.
///
/// Returns 0 when the file is (or was made) well-formatted; with `--check`,
/// returns 1 if the file would change. Uses a 4-space indent.
fn fmt_file(path: &str, check: bool) -> i32 {
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading '{}': {}", path, e);
            return 1;
        }
    };

    let formatted = code_lang::format::format_document(&src, 4);

    if formatted == src {
        if !check {
            println!("{} already formatted", path);
        }
        return 0;
    }

    if check {
        eprintln!("{}: not formatted (run `code fmt {}`)", path, path);
        return 1;
    }

    if let Err(e) = fs::write(path, &formatted) {
        eprintln!("Error writing '{}': {}", path, e);
        return 1;
    }
    println!("Formatted {}", path);
    0
}

/// Parse and execute a single source file (.code).
/// Returns 0 on success, 1 on error.
fn run_file(path: &str) -> i32 {
    let program = match module_loader::load_program_with_links(Path::new(path)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };

    let mut interp = Interpreter::new();
    match interp.execute(program) {
        Ok(()) => {
            println!("Program executed successfully.");
            0
        }
        Err(e) => {
            eprintln!("Runtime error: {}", e);
            1
        }
    }
}

/// Parse --target flag from args, defaulting to "exe".
fn parse_target_flag(args: &[String]) -> String {
    for (i, arg) in args.iter().enumerate() {
        if arg == "--target" {
            if let Some(val) = args.get(i + 1) {
                return val.to_string();
            }
        }
        if let Some(val) = arg.strip_prefix("--target=") {
            return val.to_string();
        }
    }
    "exe".to_string()
}

/// Parse and compile a single source file to LLVM IR.
/// Returns 0 on success, 1 on error.
fn build_file(path: &str, target: &str, release: bool) -> i32 {
    let program = match module_loader::load_program_with_links(Path::new(path)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };

    let module_name = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("code");

    let out_dir = Path::new("target").join("llvm");
    if let Err(e) = fs::create_dir_all(&out_dir) {
        eprintln!("Error creating {:?}: {}", out_dir, e);
        return 1;
    }

    match target {
        "ir" => build_ir(&program, module_name, &out_dir),
        "exe" => build_native(&program, module_name, &out_dir, OutputKind::Executable, release),
        "shared" => build_native(&program, module_name, &out_dir, OutputKind::Shared, release),
        "static" => build_native(&program, module_name, &out_dir, OutputKind::Static, release),
        "wasm" => build_wasm(&program, module_name, &out_dir, release),
        other => {
            eprintln!("Unknown target '{}'. Use: ir, exe, shared, static, wasm", other);
            1
        }
    }
}

fn build_ir(program: &ast::Program, module_name: &str, out_dir: &Path) -> i32 {
    let ir = match codegen::emit_llvm_ir(program, module_name) {
        Ok(ir) => ir,
        Err(e) => {
            eprintln!("Codegen error: {}", e);
            return 1;
        }
    };
    let out_path = out_dir.join(format!("{}.ll", module_name));
    if let Err(e) = fs::write(&out_path, ir) {
        eprintln!("Error writing {:?}: {}", out_path, e);
        return 1;
    }
    println!("Wrote LLVM IR to {}", out_path.display());
    0
}

enum OutputKind {
    Executable,
    Shared,
    Static,
}

/// The C runtime bridge source for native module loading in compiled programs.
const RUNTIME_NATIVE_C: &str = include_str!("runtime_native.c");

fn build_native(
    program: &ast::Program,
    module_name: &str,
    out_dir: &Path,
    kind: OutputKind,
    release: bool,
) -> i32 {
    let build_target = match kind {
        OutputKind::Executable => codegen::BuildTarget::Exe,
        OutputKind::Shared => codegen::BuildTarget::Shared,
        OutputKind::Static => codegen::BuildTarget::Static,
    };

    // Tag intermediate object files with the target kind so that concurrent
    // builds of the same source for different targets don't race on them.
    let kind_tag = match kind {
        OutputKind::Executable => "exe",
        OutputKind::Shared => "shared",
        OutputKind::Static => "static",
    };

    let obj_path = out_dir.join(format!("{}.{}.o", module_name, kind_tag));
    let _has_native = match codegen::emit_object_file(program, module_name, &obj_path, build_target, release) {
        Ok(flag) => flag,
        Err(e) => {
            eprintln!("Codegen error: {}", e);
            return 1;
        }
    };

    // Always compile+link the C bridge runtime. Besides the native-module
    // dispatch helpers (used only when the program imports native modules), it
    // defines `__value_to_cstr`, which codegen emits for the polymorphic `+`
    // operator — so every native build needs it, native imports or not.
    let mut extra_obj_paths: Vec<PathBuf> = Vec::new();
    let mut extra_flags: Vec<&str> = Vec::new();

    // Static libraries only archive the primary object (link_static ignores
    // extras); the bridge is resolved when the .a is finally linked.
    if !matches!(kind, OutputKind::Static) {
        // Per-module, per-target names so concurrent builds don't race.
        let rt_c_path = out_dir.join(format!("__code_runtime_native_{}_{}.c", module_name, kind_tag));
        let rt_o_path = out_dir.join(format!("__code_runtime_native_{}_{}.o", module_name, kind_tag));
        if let Err(e) = fs::write(&rt_c_path, RUNTIME_NATIVE_C) {
            eprintln!("Error writing runtime C source: {}", e);
            return 1;
        }
        if let Err(e) = compile_c_runtime(&rt_c_path, &rt_o_path) {
            eprintln!("Error compiling native runtime: {}", e);
            return 1;
        }
        extra_obj_paths.push(rt_o_path);
        extra_flags.push("-ldl");
        extra_flags.push("-lpthread");
    }

    let extra_obj_refs: Vec<&Path> = extra_obj_paths.iter().map(|p| p.as_path()).collect();

    let (out_path, link_result) = match kind {
        OutputKind::Executable => {
            let p = out_dir.join(module_name);
            let r = linker::link_executable(&obj_path, &p, &extra_obj_refs, &extra_flags);
            (p, r)
        }
        OutputKind::Shared => {
            let p = out_dir.join(format!("lib{}.so", module_name));
            let r = linker::link_shared(&obj_path, &p, &extra_obj_refs, &extra_flags);
            (p, r)
        }
        OutputKind::Static => {
            let p = out_dir.join(format!("lib{}.a", module_name));
            let r = linker::link_static(&obj_path, &p);
            (p, r)
        }
    };

    // Clean up intermediate files.
    let _ = fs::remove_file(&obj_path);
    for p in &extra_obj_paths {
        let _ = fs::remove_file(p);
        let _ = fs::remove_file(p.with_extension("c"));
    }

    match link_result {
        Ok(()) => {
            println!("Wrote {}", out_path.display());
            0
        }
        Err(e) => {
            eprintln!("Link error: {}", e);
            1
        }
    }
}

/// Compile a C source file to an object file using `cc`.
fn compile_c_runtime(c_path: &Path, o_path: &Path) -> Result<(), String> {
    // -fPIC so the bridge object can be linked into shared libraries as well as
    // executables.
    let output = std::process::Command::new("cc")
        .args(["-c", "-O2", "-fPIC", "-o"])
        .arg(o_path)
        .arg(c_path)
        .output()
        .map_err(|e| format!("Failed to run cc: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cc failed: {}", stderr.trim()));
    }
    Ok(())
}

fn build_wasm(program: &ast::Program, module_name: &str, out_dir: &Path, release: bool) -> i32 {
    let obj_path = out_dir.join(format!("{}.wasm.o", module_name));
    if let Err(e) = codegen::emit_wasm_object(program, module_name, &obj_path, release) {
        eprintln!("Codegen error: {}", e);
        return 1;
    }

    let out_path = out_dir.join(format!("{}.wasm", module_name));
    let link_result = linker::link_wasm(&obj_path, &out_path);

    // Clean up intermediate .o
    let _ = fs::remove_file(&obj_path);

    match link_result {
        Ok(()) => {
            println!("Wrote {}", out_path.display());
            0
        }
        Err(e) => {
            eprintln!("Link error: {}", e);
            1
        }
    }
}

/// Run all .code test files in the tests/ directory.
/// Files starting with `fail_` are expected to error.
/// Returns 0 if all tests pass, 1 otherwise.
fn run_tests() -> i32 {
    let test_dir = Path::new("tests");
    if !test_dir.is_dir() {
        eprintln!("Error: tests/ directory not found");
        return 1;
    }

    let mut entries: Vec<_> = match fs::read_dir(test_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "code")
                    .unwrap_or(false)
            })
            .collect(),
        Err(e) => {
            eprintln!("Error reading tests/: {}", e);
            return 1;
        }
    };

    entries.sort_by_key(|e| e.file_name());

    if entries.is_empty() {
        println!("No .code test files found in tests/");
        return 0;
    }

    println!("Running {} test(s)...\n", entries.len());

    let mut passed = 0u32;
    let mut failed = 0u32;

    for entry in &entries {
        let path = entry.path();
        let filename = entry.file_name();
        let name = filename.to_string_lossy();
        let expect_fail = name.starts_with("fail_");

        let result = execute_file(&path);

        match (result, expect_fail) {
            (Ok(()), false) => {
                println!("  [PASS] {}", name);
                passed += 1;
            }
            (Ok(()), true) => {
                println!("  [FAIL] {} (expected failure, but succeeded)", name);
                failed += 1;
            }
            (Err(_), true) => {
                println!("  [PASS] {} (expected failure)", name);
                passed += 1;
            }
            (Err(e), false) => {
                println!("  [FAIL] {} — {}", name, e);
                failed += 1;
            }
        }
    }

    println!();
    println!("Tests passed: {}", passed);
    println!("Tests failed: {}", failed);

    if failed > 0 { 1 } else { 0 }
}

/// Parse and execute a source file, returning Ok or an error message.
fn execute_file(path: &Path) -> Result<(), String> {
    let program = module_loader::load_program_with_links(path)?;

    let mut interp = Interpreter::new();
    interp.execute(program)
}

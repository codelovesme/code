pub mod ast;
#[cfg(feature = "llvm")]
pub mod codegen;
pub mod interpreter;
pub mod lexer;
pub mod loader;
pub mod parser;
pub mod value;

use std::path::Path;

use interpreter::Environment;
use loader::{FilesystemResolver, NoModules};

/// Run a program from a file. The path is not just a convenience: `link`
/// resolves relative to the file doing the linking, so a program that links
/// anything can only be run through an entry point that knows where it lives.
pub fn run_file(path: &Path) -> Result<Environment, String> {
    let program = loader::load(&path.display().to_string(), &FilesystemResolver)?;
    interpreter::run(&program)
}

/// Run a program from source text alone, with no module story — `link` in it
/// is refused rather than resolved. This is what a host without a filesystem
/// uses (`crates/code-wasm`, and so the playground).
pub fn run_source(src: &str) -> Result<Environment, String> {
    let resolver = NoModules {
        entry_identity: "<source>".to_string(),
        entry_text: src.to_string(),
    };
    let program = loader::load("<source>", &resolver)?;
    interpreter::run(&program)
}

/// Renders a run's final bindings the way `code run` prints them — shared by
/// the CLI and by tests that need to compare the interpreter's output
/// against the compiled binary's stdout, so the two can never silently
/// drift apart.
pub fn format_bindings(env: &Environment) -> String {
    env.iter_in_order()
        .map(|(name, value)| format!("{name} = {value}\n"))
        .collect()
}

#[cfg(feature = "llvm")]
mod compile {
    use std::path::Path;
    use std::process::Command;

    use crate::ast::Program;
    use crate::codegen;
    use crate::loader::{self, FilesystemResolver};

    /// Runtime support functions (`code_number`, `code_array`, ...) that
    /// every compiled program links against — embedded so `code build`
    /// works regardless of the current directory or install location.
    const RUNTIME_C: &str = include_str!("runtime.c");

    /// Compile a program from a file into a standalone executable at
    /// `exe_path`, via LLVM object codegen (see `codegen.rs`) linked against
    /// the embedded C runtime through the system `cc`. Takes a path for the
    /// same reason `run_file` does — `link` resolves relative to it.
    pub fn compile_file(source_path: &Path, exe_path: &Path) -> Result<(), String> {
        let program: Program =
            loader::load(&source_path.display().to_string(), &FilesystemResolver)?;

        let obj_path = exe_path.with_extension("o");
        codegen::compile_to_object(&program, &obj_path)?;

        let runtime_c_path = exe_path.with_extension("runtime.c");
        std::fs::write(&runtime_c_path, RUNTIME_C).map_err(|e| format!("write runtime.c: {e}"))?;

        let link_result = Command::new("cc")
            .arg(&obj_path)
            .arg(&runtime_c_path)
            .arg("-lm")
            .arg("-o")
            .arg(exe_path)
            .status();

        let _ = std::fs::remove_file(&obj_path);
        let _ = std::fs::remove_file(&runtime_c_path);

        match link_result {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(format!("cc failed with {status}")),
            Err(e) => Err(format!("failed to run cc: {e}")),
        }
    }
}

#[cfg(feature = "llvm")]
pub use compile::compile_file;

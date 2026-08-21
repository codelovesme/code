pub mod ast;
#[cfg(feature = "llvm")]
pub mod codegen;
pub mod interpreter;
pub mod lexer;
pub mod parser;
pub mod value;

use interpreter::Environment;

/// Lex, parse, and run a full source string — the one entry point every
/// caller (CLI, tests, and eventually anything else) should go through.
pub fn run_source(src: &str) -> Result<Environment, String> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens)?;
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

    use crate::{ast::Program, codegen, lexer, parser};

    /// Runtime support functions (`code_number`, `code_array`, ...) that
    /// every compiled program links against — embedded so `code build`
    /// works regardless of the current directory or install location.
    const RUNTIME_C: &str = include_str!("runtime.c");

    /// Lex, parse, and compile a full source string into a standalone
    /// executable at `exe_path`, via LLVM object codegen (see `codegen.rs`)
    /// linked against the embedded C runtime through the system `cc`.
    pub fn compile_source(src: &str, exe_path: &Path) -> Result<(), String> {
        let tokens = lexer::tokenize(src)?;
        let program: Program = parser::parse(&tokens)?;

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
pub use compile::compile_source;

pub mod ast;
#[cfg(feature = "llvm")]
pub mod codegen;
pub mod interpreter;
pub mod lexer;
pub mod loader;
#[cfg(feature = "native-modules")]
pub mod native;
pub mod parser;
pub mod span;
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

#[cfg(feature = "llvm")]
mod compile {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::ast::{NativeFormat, Program, Stmt};
    use crate::codegen;
    use crate::loader::{self, FilesystemResolver};

    /// Runtime support functions (`code_number`, `code_array`, ...) that
    /// every compiled program links against — embedded so `code build`
    /// works regardless of the current directory or install location.
    const RUNTIME_C: &str = include_str!("runtime.c");
    /// The native-module ABI header `runtime.c` `#include`s — embedded
    /// alongside it and written next to it so that `#include` resolves
    /// regardless of where `code build` runs from.
    const CODE_ABI_H: &str = include_str!("code_abi.h");

    /// Distinguishes concurrent builds *within* one process; the pid
    /// distinguishes them across processes. See `scratch_dir`.
    static BUILD_SEQ: AtomicU64 = AtomicU64::new(0);

    /// A private directory for one build's intermediate files.
    ///
    /// Unique per build, and that is load-bearing rather than tidiness:
    /// `runtime.c` does `#include "code_abi.h"`, so the header has to sit
    /// beside it under exactly that name. Writing both next to `exe_path`
    /// — as this used to — meant two `code build` runs sharing an output
    /// directory raced, and whichever finished first deleted the header the
    /// other was still compiling against (`fatal error: code_abi.h: No such
    /// file or directory`). A `make -j` over several programs, or a test
    /// harness compiling fixtures in parallel, hits that for real.
    ///
    /// Keeping the object file here too means a build no longer leaves
    /// `<name>.o`, `<name>.runtime.c`, and `code_abi.h` beside the binary it
    /// produced.
    fn scratch_dir() -> Result<PathBuf, String> {
        let dir = std::env::temp_dir().join(format!(
            "code-build-{}-{}",
            std::process::id(),
            BUILD_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        // A leftover directory from a previous run that crashed before its
        // cleanup is harmless: every file in it is overwritten below.
        fs::create_dir_all(&dir).map_err(|e| format!("create build directory: {e}"))?;
        Ok(dir)
    }

    /// Compile a program from a file into a standalone executable at
    /// `exe_path`, via LLVM object codegen (see `codegen.rs`) linked against
    /// the embedded C runtime through the system `cc`. Takes a path for the
    /// same reason `run_file` does — `link` resolves relative to it.
    pub fn compile_file(source_path: &Path, exe_path: &Path) -> Result<(), String> {
        let program: Program =
            loader::load(&source_path.display().to_string(), &FilesystemResolver)?;

        let scratch = scratch_dir()?;
        let obj_path = scratch.join("program.o");
        let runtime_c_path = scratch.join("runtime.c");
        let abi_h_path = scratch.join("code_abi.h");

        // Every path below is inside `scratch`, so the whole directory can be
        // removed as one on the way out, on success and failure alike.
        let result = (|| {
            codegen::compile_to_object(&program, &obj_path)?;
            fs::write(&runtime_c_path, RUNTIME_C).map_err(|e| format!("write runtime.c: {e}"))?;
            fs::write(&abi_h_path, CODE_ABI_H).map_err(|e| format!("write code_abi.h: {e}"))?;

            // Every `.a` static module `link`ed in this program (see
            // `ast::NativeFormat::Static`) — appended after `runtime_c_path`
            // so its unresolved `code_number`/etc. references are satisfied
            // by that plain (non-archive) object regardless of `.a`-vs-.o
            // ordering quirks, while `obj_path`'s own references to
            // `<prefix>_code_module_dispatch` pull the archive member in.
            let static_modules: Vec<&str> = program
                .statements
                .iter()
                .filter_map(|stmt| match stmt {
                    Stmt::ImportNative {
                        path,
                        format: NativeFormat::Static { .. },
                        ..
                    } => Some(path.as_str()),
                    _ => None,
                })
                .collect();

            let link_result = Command::new("cc")
                .arg(&obj_path)
                .arg(&runtime_c_path)
                .args(&static_modules)
                .arg("-lm")
                .arg("-ldl")
                .arg("-o")
                .arg(exe_path)
                .status();

            match link_result {
                Ok(status) if status.success() => Ok(()),
                Ok(status) => Err(format!("cc failed with {status}")),
                Err(e) => Err(format!("failed to run cc: {e}")),
            }
        })();

        let _ = fs::remove_dir_all(&scratch);
        result
    }
}

#[cfg(feature = "llvm")]
pub use compile::compile_file;

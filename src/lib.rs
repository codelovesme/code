pub mod ast;
#[cfg(feature = "llvm")]
pub mod codegen;
pub mod handlers;
pub mod interpreter;
pub mod lexer;
pub mod loader;
#[cfg(feature = "install")]
pub mod module_install;
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
    use crate::codegen::{self, BuildTarget};
    use crate::loader::{self, FilesystemResolver};

    /// Runtime support functions (`code_number`, `code_array`, ...) that
    /// every compiled program links against — embedded so `code build`
    /// works regardless of the current directory or install location.
    const RUNTIME_C: &str = include_str!("runtime.c");
    /// The native-module ABI header `runtime.c` `#include`s — embedded
    /// alongside it and written next to it so that `#include` resolves
    /// regardless of where `code build` runs from.
    const CODE_ABI_H: &str = include_str!("code_abi.h");
    /// Freestanding libc-shaped helpers used only by the wasm runtime build.
    const WASM_SHIM_H: &str = include_str!("wasm_shim.h");

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

    /// Compile a program from a file into the artifact named by `target`
    /// at `out_path`, via LLVM object codegen (see `codegen.rs`). The
    /// generated object is byte-identical for all three native containers —
    /// codegen asks for `RelocMode::PIC`, which `-shared` needs anyway — so
    /// the target changes only the link step: `cc` for `Exe`, `cc -shared`
    /// for `Shared`, `ar rcs` for `Static` (see `docs/todo/build-targets.md`
    /// for why these are deliberately plain containers, not module-ABI
    /// libraries). Takes a path for the same reason `run_file` does —
    /// `link` resolves relative to it.
    ///
    /// `release` selects the LLVM optimization level — off by default, `-O2`
    /// when asked. It changes only how the object is compiled, never what the
    /// program means, so no fixture's output depends on it.
    pub fn compile_file(
        source_path: &Path,
        target: BuildTarget,
        out_path: &Path,
        release: bool,
    ) -> Result<(), String> {
        let program: Program =
            loader::load(&source_path.display().to_string(), &FilesystemResolver)?;

        let scratch = scratch_dir()?;
        let obj_path = scratch.join("program.o");
        let runtime_c_path = scratch.join("runtime.c");
        let abi_h_path = scratch.join("code_abi.h");
        let wasm_shim_path = scratch.join("wasm_shim.h");
        let runtime_obj_path = scratch.join("runtime.o");

        // Every path below is inside `scratch`, so the whole directory can be
        // removed as one on the way out, on success and failure alike.
        let result = (|| {
            codegen::compile_to_object(&program, target, &obj_path, release)?;

            // `Static` never links against the C runtime — there is no link
            // step beyond archiving the object — so skip writing the sources
            // it would not even read.
            if !matches!(target, BuildTarget::Static) {
                fs::write(&runtime_c_path, RUNTIME_C)
                    .map_err(|e| format!("write runtime.c: {e}"))?;
                fs::write(&abi_h_path, CODE_ABI_H).map_err(|e| format!("write code_abi.h: {e}"))?;
            }

            if target == BuildTarget::Wasm {
                fs::write(&wasm_shim_path, WASM_SHIM_H)
                    .map_err(|e| format!("write wasm_shim.h: {e}"))?;
                compile_wasm_runtime(
                    &runtime_c_path,
                    &wasm_shim_path,
                    &abi_h_path,
                    &runtime_obj_path,
                )?;
                return link_wasm(&obj_path, &runtime_obj_path, out_path);
            }

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

            match target {
                BuildTarget::Exe => {
                    cc_link(&[&obj_path, &runtime_c_path], &static_modules, out_path)
                }
                BuildTarget::Shared => {
                    cc_link_shared(&[&obj_path, &runtime_c_path], &static_modules, out_path)
                }
                BuildTarget::Static => ar_archive(&obj_path, out_path),
                // Refused earlier, in `compile_to_object` — unreachable.
                BuildTarget::Wasm => unreachable!("wasm refused before codegen"),
            }
        })();

        let _ = fs::remove_dir_all(&scratch);
        result
    }

    /// Links a standalone executable: the program object plus the embedded
    /// C runtime, any statically `link`ed modules, and the usual system
    /// libraries.
    fn cc_link(
        obj_paths: &[&Path],
        static_modules: &[&str],
        out_path: &Path,
    ) -> Result<(), String> {
        run_command(
            Command::new("cc")
                .args(obj_paths)
                .args(static_modules)
                .arg("-lm")
                .arg("-ldl")
                .arg("-o")
                .arg(out_path),
            "cc",
        )
    }

    /// Links a shared library: the same inputs as `cc_link` under
    /// `-shared`. Statically `link`ed modules come along too — a `.a`
    /// whose members are position-independent links into a `.so` exactly
    /// as it does into an executable (and one that isn't produces the
    /// ordinary linker relocation error, which names the offending
    /// member). `-fPIC` is passed for clarity even though codegen already
    /// emits PIC objects: it documents intent and guards against a future
    /// codegen change silently producing a non-loadable library.
    fn cc_link_shared(
        obj_paths: &[&Path],
        static_modules: &[&str],
        out_path: &Path,
    ) -> Result<(), String> {
        run_command(
            Command::new("cc")
                .arg("-shared")
                .arg("-fPIC")
                .args(obj_paths)
                .args(static_modules)
                .arg("-lm")
                .arg("-ldl")
                .arg("-o")
                .arg(out_path),
            "cc",
        )
    }

    /// Archives the program object into a static library. No runtime, no
    /// system libraries — consumers of the archive supply their own.
    fn ar_archive(obj_path: &Path, out_path: &Path) -> Result<(), String> {
        run_command(
            Command::new("ar").arg("rcs").arg(out_path).arg(obj_path),
            "ar",
        )
    }

    fn compile_wasm_runtime(
        runtime_c_path: &Path,
        shim_path: &Path,
        abi_h_path: &Path,
        runtime_obj_path: &Path,
    ) -> Result<(), String> {
        run_command(
            Command::new("clang")
                .arg("--target=wasm32-unknown-unknown")
                .arg("-nostdlib")
                .arg("-fno-builtin")
                .arg("-DCODE_WASM")
                .arg("-include")
                .arg(shim_path)
                .arg("-I")
                .arg(abi_h_path.parent().unwrap_or_else(|| Path::new(".")))
                .arg("-c")
                .arg(runtime_c_path)
                .arg("-o")
                .arg(runtime_obj_path),
            "clang (wasm runtime)",
        )
    }

    fn link_wasm(obj_path: &Path, runtime_obj_path: &Path, out_path: &Path) -> Result<(), String> {
        let mut linker = Command::new("wasm-ld");
        if linker.output().is_err() {
            let sysroot = Command::new("rustc")
                .args(["--print", "sysroot"])
                .output()
                .map_err(|e| format!("failed to find wasm linker: {e}"))?;
            if !sysroot.status.success() {
                return Err("failed to find wasm linker: rustc --print sysroot failed".to_string());
            }
            let host = Command::new("rustc")
                .args(["-vV"])
                .output()
                .map_err(|e| format!("failed to find wasm linker host: {e}"))?;
            let host = String::from_utf8_lossy(&host.stdout)
                .lines()
                .find_map(|line| line.strip_prefix("host: "))
                .map(str::to_owned)
                .ok_or_else(|| "failed to find wasm linker host triple".to_string())?;
            let rust_lld = PathBuf::from(String::from_utf8_lossy(&sysroot.stdout).trim())
                .join("lib/rustlib")
                .join(host)
                .join("bin/rust-lld");
            if !rust_lld.is_file() {
                return Err(
                    "no wasm linker found — install lld, or use a rustup-managed toolchain"
                        .to_string(),
                );
            }
            linker = Command::new(rust_lld);
            linker.arg("-flavor").arg("wasm");
        }
        run_command(
            linker
                .arg("--no-entry")
                .arg("--export=main")
                .arg("--export-memory")
                .arg("--allow-undefined")
                .arg(obj_path)
                .arg(runtime_obj_path)
                .arg("-o")
                .arg(out_path),
            "wasm linker",
        )
    }

    /// Runs a linker/archiver and reports a failed status or spawn error as
    /// itself, naming the tool — a missing `ar` should read as "failed to
    /// run ar", not as some downstream mystery.
    fn run_command(cmd: &mut Command, tool: &str) -> Result<(), String> {
        let status = cmd
            .status()
            .map_err(|e| format!("failed to run {tool}: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("{tool} failed with {status}"))
        }
    }
}

#[cfg(feature = "llvm")]
pub use codegen::BuildTarget;
#[cfg(feature = "llvm")]
pub use compile::compile_file;

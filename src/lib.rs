pub mod ast;
#[cfg(feature = "llvm")]
pub mod codegen;
pub mod format;
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
pub mod verify;

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
    /// The language's own half of a page: the four functions a host with no
    /// operating system has to supply, and the wiring that lets a page fire a
    /// particle back.
    ///
    /// Embedded for the same reason as the three above, and belonging to a
    /// build for a stronger one: a `.wasm` and the JavaScript that answers its
    /// imports are one artifact in two pieces. The compiler knows which
    /// runtime it linked and which modules went in, so the half that answers
    /// them cannot be a version behind — it came out of the same binary in
    /// the same second.
    const WEB_RUNTIME_JS: &str = include_str!("../web/runtime.mjs");
    /// Where the module halves go, verbatim, inside `PARTS`.
    const WEB_PARTS_MARK: &str = "//__CODE_WEB_PARTS__";
    /// Each browser module's own half, by the prefix its `.a` exports under.
    ///
    /// A module is two pieces of code and keeps them together — its
    /// `page.mjs` sits beside its Rust — so this table is a list of which
    /// modules have a browser half at all, not a second implementation of
    /// them.
    ///
    /// Embedded rather than read out of the archive the program linked, which
    /// is the honest limit here: **a module from outside this repository
    /// cannot bring its own half.** Putting it inside the `.a` was tried and
    /// makes the wasm linker warn on every build ("archive member is neither
    /// Wasm object file nor LLVM bitcode"); the way out is a second published
    /// asset beside the archive, which is release, install and lockfile work
    /// nobody needs yet.
    const WEB_MODULE_PARTS: &[(&str, &str)] = &[
        (
            "console",
            include_str!("../crates/modules/console/page.mjs"),
        ),
        ("dom", include_str!("../crates/modules/dom/page.mjs")),
        (
            "net_client",
            include_str!("../crates/modules/net_client/page.mjs"),
        ),
        ("router", include_str!("../crates/modules/router/page.mjs")),
        (
            "storage",
            include_str!("../crates/modules/storage/page.mjs"),
        ),
        ("timer", include_str!("../crates/modules/timer/page.mjs")),
    ];

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

            // Every `.a` static module `link`ed in this program (see
            // `ast::NativeFormat::Static`) — appended after `runtime_c_path`
            // so its unresolved `code_number`/etc. references are satisfied
            // by that plain (non-archive) object regardless of `.a`-vs-.o
            // ordering quirks, while `obj_path`'s own references to
            // `<prefix>_code_module_dispatch` pull the archive member in.
            let mut static_modules: Vec<&str> = Vec::new();
            let mut prefixes: Vec<(&str, &str)> = Vec::new();
            for stmt in &program.statements {
                if let Stmt::ImportNative {
                    path,
                    format: NativeFormat::Static { prefix, .. },
                    ..
                } = stmt
                {
                    // Two *different* archives answering to the same prefix
                    // define the same symbols, and one would win silently.
                    // The linker used to catch that on its own; a wasm link
                    // no longer lets it (see `link_wasm`), so the check
                    // belongs here, where it can name both files. The same
                    // archive linked twice under two names is one archive
                    // and not a clash.
                    if let Some((other, _)) = prefixes
                        .iter()
                        .find(|(other, p)| p == &prefix.as_str() && *other != path.as_str())
                    {
                        return Err(format!(
                            "cannot link '{path}' beside '{other}': both name their exports \
                             '{prefix}_code_module_*', so one would quietly replace the other \
                             — a .a module's prefix must be unique among the archives one \
                             program links (see code_abi.h's \".a static modules\" section)"
                        ));
                    }
                    prefixes.push((path.as_str(), prefix.as_str()));
                    if !static_modules.contains(&path.as_str()) {
                        static_modules.push(path.as_str());
                    }
                }
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
                link_wasm(&obj_path, &runtime_obj_path, &static_modules, out_path)?;
                return write_web_host(out_path, &prefixes);
            }

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
                // The runtime locks each module's inbound ring, because a
                // module may push from a thread of its own (see
                // `runtime.c`'s "Inbound" section). Glibc 2.34 and later put
                // the mutex calls in libc itself, so this is a no-op there;
                // it is what makes an older one link.
                .arg("-pthread")
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
                // See `cc_link`.
                .arg("-pthread")
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

    /// Links the one `.wasm`: the program, the runtime, and every `.a`
    /// module the program linked — all of it in a single module, with
    /// nothing left to load.
    /// Writes `host.mjs` beside the module just built.
    ///
    /// A `.wasm` cannot reach a page on its own: every browser module leaves
    /// a function undefined for the page to fill in, and so does the language
    /// itself, which has no operating system to ask for a clock. This is
    /// those functions. Emitting it here rather than making it something to
    /// install is what keeps the two halves the same age — nothing fetches
    /// it, nothing pins it, and it cannot drift from the runtime it answers.
    ///
    /// Overwritten every build, and named the same every time so a page's
    /// `import` never has to change. It is output, not source: a project
    /// ignores it the way it ignores the `.wasm` beside it.
    fn write_web_host(out_path: &Path, linked: &[(&str, &str)]) -> Result<(), String> {
        let prefixes: Vec<&str> = linked.iter().map(|(_, prefix)| *prefix).collect();
        let beside = out_path.parent().unwrap_or(Path::new("."));
        let host = beside.join("host.mjs");
        fs::write(&host, web_host_source(&prefixes))
            .map_err(|e| format!("write {}: {e}", host.display()))
    }

    /// The page's half for a program that linked `prefixes`.
    ///
    /// Only the halves of the modules actually linked. An extra import is
    /// harmless — a module fails to instantiate for one it *needs* and does
    /// not have, never for one it was handed anyway — so this is not about
    /// correctness but about an application not carrying the browser half of
    /// a module it never mentioned.
    ///
    /// A prefix with no half is simply not one of these modules: every `.a` a
    /// wasm program links comes through here, and most of them have nothing
    /// to say to a page.
    pub fn web_host_source(prefixes: &[&str]) -> String {
        let parts: Vec<&str> = prefixes
            .iter()
            .filter_map(|prefix| {
                WEB_MODULE_PARTS
                    .iter()
                    .find(|(name, _)| name == prefix)
                    .map(|(_, js)| *js)
            })
            .collect();
        WEB_RUNTIME_JS.replace(WEB_PARTS_MARK, &parts.join(",\n"))
    }

    fn link_wasm(
        obj_path: &Path,
        runtime_obj_path: &Path,
        static_modules: &[&str],
        out_path: &Path,
    ) -> Result<(), String> {
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
                // How a page calls back in. A program that draws nothing
                // never fires anything, so exporting these costs a few table
                // entries and nothing else.
                .arg("--export=code_event_fire")
                .arg("--export=code_event_ask")
                .arg("--export=code_event_text")
                .arg("--export=code_event_text_capacity")
                .arg("--export-memory")
                .arg("--allow-undefined")
                // A wasm module is something a browser downloads, and the
                // debug information a Rust module archive carries is most of
                // the file: 624 KB of one measured module was 25 KB of code
                // and the rest DWARF. Stripped here rather than left to the
                // person deploying it, who would have to know it was there.
                .arg("--strip-debug")
                // Rust puts its panic handler and unwinding personality in
                // every archive it produces, so any two Rust modules in one
                // program define them twice — a `no_std` one and a `std` one
                // above all, which is exactly what a web application links.
                // Nothing can be done about it from inside a module: a
                // `staticlib` must carry a panic handler, and the program
                // that links them is not Rust and has none to offer. Either
                // definition does the same thing, so the duplicate is
                // allowed rather than fatal. What this would otherwise have
                // caught — two modules sharing a prefix — is caught above by
                // name, with a better error than the linker's.
                .arg("--allow-multiple-definition")
                .arg(obj_path)
                .arg(runtime_obj_path)
                .args(static_modules)
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
#[cfg(feature = "llvm")]
pub use compile::web_host_source;

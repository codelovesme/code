//! Compiles the C runtime once, optimised, for `code build` to embed
//! (ticket 111).
//!
//! Every compiled program links `src/runtime.c`. It used to be compiled
//! from source inside every build, with no optimisation — most of a small
//! program's build time, and every program ran on an unoptimised runtime.
//! Here it is compiled when `code` itself is built, `-O2` — for this
//! machine's executables and shared libraries, and for wasm32 — and
//! `src/lib.rs` embeds the objects (`include_bytes!`).
//!
//! A compiler that is missing (no `clang` with wasm32 on a machine running
//! `cargo install`) is not an error: that object is left empty, and
//! `code build` compiles the runtime itself the first time and keeps it in
//! the shared cache. The flags below must stay the same as `lib.rs`'s
//! `EXE_RUNTIME_FLAGS`, `SHARED_RUNTIME_FLAGS` and `WASM_RUNTIME_FLAGS`,
//! which that fallback uses.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    for file in [
        "build.rs",
        "src/runtime.c",
        "src/code_abi.h",
        "src/wasm_shim.h",
    ] {
        println!("cargo:rerun-if-changed={file}");
    }
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let exe_obj = out.join("runtime-exe.o");
    let shared_obj = out.join("runtime-shared.o");
    let wasm_obj = out.join("runtime-wasm32.o");
    // Empty means "not embedded"; `include_bytes!` needs the files either way.
    for obj in [&exe_obj, &shared_obj, &wasm_obj] {
        fs::write(obj, b"").expect("write an empty runtime object");
    }
    // Only `code build` (the `llvm` feature) links programs; the
    // interpreter-only builds (code-wasm, the npm package) never need it.
    if env::var_os("CARGO_FEATURE_LLVM").is_none() {
        return;
    }
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let runtime = src.join("runtime.c");

    // The host objects only when `code` runs where it is built: a
    // cross-compiled `code` would otherwise embed the wrong machine's code.
    // Two of them, as the runtime was compiled before: position-independent
    // *executable* code for an executable (calls inside the runtime stay
    // direct — `-fPIC` there measured a third slower), and `-fPIC` for a
    // shared library, keeping its symbols' resolution as it was.
    if env::var("TARGET").ok() == env::var("HOST").ok() {
        for (pic, obj) in [("-fPIE", &exe_obj), ("-fPIC", &shared_obj)] {
            let mut cc = Command::new("cc");
            cc.args(["-O2", pic, "-pthread", "-c"])
                .arg("-I")
                .arg(&src)
                .arg(&runtime)
                .arg("-o")
                .arg(obj);
            compile(cc, obj, "cc");
        }
    }

    let shim = src.join("wasm_shim.h");
    let mut clang = Command::new("clang");
    clang
        .args([
            "--target=wasm32-unknown-unknown",
            "-O2",
            "-nostdlib",
            "-fno-builtin",
            "-DCODE_WASM",
        ])
        .arg("-include")
        .arg(&shim)
        .arg("-I")
        .arg(&src)
        .arg("-c")
        .arg(&runtime)
        .arg("-o")
        .arg(&wasm_obj);
    compile(clang, &wasm_obj, "clang (wasm32)");
}

fn compile(mut command: Command, obj: &Path, what: &str) {
    let ok = command.status().map(|s| s.success()).unwrap_or(false);
    if !ok {
        let _ = fs::write(obj, b"");
        println!(
            "cargo:warning=could not compile the runtime with {what}; `code build` will \
             compile it on first use and cache it"
        );
    }
}

//! Compiles the vendored `runtime.c` and links it into whatever final
//! artifact depends on this crate — a Rust module's `cdylib` gets a real
//! `code_release` (and every other `code_abi.h` function) this way, the
//! same role `#include "runtime.c"` plays for a C module. See `vendor/`'s
//! doc comment for how these two files are kept in sync with `src/` in the
//! main `code` repo.

fn main() {
    cc::Build::new()
        .file("vendor/runtime.c")
        .include("vendor")
        // Renamed at the preprocessor level so `src/lib.rs` can re-export it
        // under its real name from a function rustc treats as part of the
        // crate rather than this archive — see `code_release`'s doc comment
        // in `src/lib.rs` for why that distinction matters for a `cdylib`.
        .define("code_release", "code_native_vendored_release")
        .compile("code_native_runtime");
    println!("cargo:rerun-if-changed=vendor/runtime.c");
    println!("cargo:rerun-if-changed=vendor/code_abi.h");
}

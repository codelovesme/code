//! Compiles the vendored `runtime.c` and links it into whatever final
//! artifact depends on this crate — a Rust module's `cdylib` gets a real
//! `code_release` (and every other `code_abi.h` function) this way, the
//! same role `#include "runtime.c"` plays for a C module. See `vendor/`'s
//! doc comment for how these two files are kept in sync with `src/` in the
//! main `code` repo.

fn main() {
    // A `.a` module brings no runtime: it links into the host binary, which
    // already has exactly one. Compiling the vendored copy in as well is a
    // link error, not a waste — `multiple definition of 'code_release'`, and
    // forty more. See this crate's README.
    if std::env::var_os("CARGO_FEATURE_STATIC_MODULE").is_some() {
        println!("cargo:rerun-if-changed=vendor/runtime.c");
        println!("cargo:rerun-if-changed=vendor/code_abi.h");
        return;
    }
    cc::Build::new()
        .file("vendor/runtime.c")
        .include("vendor")
        // Renamed at the preprocessor level so `src/lib.rs` can re-export it
        // under its real name from a function rustc treats as part of the
        // crate rather than this archive — see `code_release`'s doc comment
        // in `src/lib.rs` for why that distinction matters for a `cdylib`.
        .define("code_release", "code_native_vendored_release")
        // `code_abi.h` item 14: a Rust module's base is what the host hands
        // over to *this crate* (`declare_base!`, `ask_base`), not to the
        // vendored runtime, which has no program to answer from — renamed
        // so the two never meet in one symbol table.
        .define("code_module_set_base", "code_native_vendored_set_base")
        .define("code_base_ask", "code_native_vendored_base_ask")
        .compile("code_native_runtime");
    println!("cargo:rerun-if-changed=vendor/runtime.c");
    println!("cargo:rerun-if-changed=vendor/code_abi.h");
}

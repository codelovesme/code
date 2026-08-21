//! Thin browser bridge around the interpreter (LLVM/`code build` is
//! native-only — see the root crate's `llvm` feature — so this only ever
//! wraps `code::run_source`). Powers `site/`'s playground.

use wasm_bindgen::prelude::*;

/// Runs `src` and returns either the bindings dump (matching `code run`'s
/// stdout exactly, via the same `format_bindings`) or `"error: ..."` — a
/// single string return keeps the JS side trivial for now.
#[wasm_bindgen]
pub fn run(src: &str) -> String {
    match code::run_source(src) {
        Ok(env) => code::format_bindings(&env),
        Err(e) => format!("error: {e}"),
    }
}

//! The `clipboard` native module — the reader's clipboard.
//!
//! Handlers:
//!
//! - `Copy { text }` — puts `text` on the clipboard and answers
//!   `CopyResult { ok }`. `ok` is false when the browser offers no
//!   clipboard at all. A browser that refuses afterwards — outside a
//!   click, or on an insecure page — fires `CopyFailed { reason }` on its
//!   own, because the refusal comes back after the answer has gone.
//!
//! # Its own module
//!
//! The clipboard is not the page: `dom` draws, `storage` remembers, and
//! this copies. A program that wants one links one, and a module that does
//! one thing can be replaced by one that does that thing differently.
//!
//! # Where it works
//!
//! **A browser.** On a machine every handler answers an `Exception` saying
//! so: a clipboard belongs to whoever is sitting there. The module is still
//! linkable, so one application can be built both ways and ask `Linked`
//! which it is.
//!
//! For wasm it is built as an archive linked into the program:
//!
//! ```bash
//! cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
//! ```
//!
//! Its page half is in `page.mjs`, with every other browser module's — see
//! `crates/modules/browser_half.rs` for the wasm half, written once.

#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
    //! Where there is no browser, and so no clipboard to reach.
    use code_native::*;

    const NO_BROWSER: &str =
        "there is no browser here — `clipboard` is the reader's clipboard, and this program is \
         running on a machine. Ask `Linked` to find out which you are";

    #[no_mangle]
    pub extern "C" fn code_module_abi_version() -> u32 {
        CODE_ABI_VERSION
    }

    /// # Safety
    ///
    /// Both pointers must be valid for the duration of the call and laid out
    /// per `code_abi.h` — the host guarantees this on every dispatch.
    #[no_mangle]
    pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
        let particle = &*particle;
        guarded(&mut *out, "clipboard", |out| {
            match read_field_str(particle, "_class") {
                Some("Copy") => exception(out, "clipboard", NO_BROWSER),
                // A class this module does not handle answers null and does
                // not end the program: it may have been meant for something
                // else entirely.
                _ => null(out),
            }
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod page {
    include!("../../browser_half.rs");

    browser_half!(
        "clipboard",
        clipboard_code_module_abi_version,
        clipboard_code_module_dispatch
    );
}

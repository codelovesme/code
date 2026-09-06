//! The `storage` native module — what the browser remembers between visits.
//!
//! Handlers:
//!
//! - `Get { key }` — answers `GetResult { value }`, the text stored under
//!   `key`, or null when nothing is stored there.
//! - `Set { key, value }` — answers `SetResult { ok }`. `ok` is false when
//!   the browser refused it, which it does when its store is full or when a
//!   reader has turned storage off.
//! - `Remove { key }` — answers `RemoveResult { ok }`.
//!
//! # Text, and only text
//!
//! What a browser stores is a string, and this module does not pretend
//! otherwise: an application with an object to keep turns it into text with
//! the `json` module and stores that. Two modules, each doing one thing,
//! rather than a store that quietly serialises — and a stored value that
//! turns out to be unparseable is then the application's to answer, at the
//! point where it knows what it expected.
//!
//! # Where it works
//!
//! **A browser.** On a machine every handler answers an `Exception` saying
//! so. The module is still linkable there, so one application can be built
//! both ways and ask [`Linked`](../../../README.md#linked) which it is.
//!
//! A machine has a filesystem and a `fs` module for exactly this, which is
//! why this one does not fall back to it: they keep different things. What
//! the browser remembers is per reader and per site, and lives as long as
//! that reader lets it.
//!
//! For wasm it is built as an archive linked into the program:
//!
//! ```bash
//! cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
//! ```
//!
//! # What the page has to supply
//!
//! A half that answers this module's particles, in
//! [`web/host.mjs`](../../../web/README.md) — which `code build --target
//! wasm` writes beside the module it built, holding the halves of exactly
//! the modules the program linked.
//!
//! Nothing crosses as a pointer or a length: the particle goes over as JSON
//! and the answer comes back the same way, through one door shared by every
//! browser module. So this module's wasm half has nothing to do but hand the
//! particle over — see `crates/modules/browser_half.rs`, which is that half,
//! written once.
//!
//! Its wasm half is `no_std` and hand-written against `code_abi.h` — see
//! `dom`'s file for why a module meant for the browser brings no standard
//! library.

#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
    //! Where there is no browser. Every handler answers an `Exception`
    //! saying so rather than quietly keeping the value somewhere else: a
    //! program that stored something and found it gone is a worse day than
    //! one told plainly that it is not in a browser.
    use code_native::*;

    const NO_BROWSER: &str =
        "there is no browser here — `storage` is what a page remembers, and this program is \
         running on a machine. Use `fs` for a machine's files, or ask `Linked` to find out \
         which you are";

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
        guarded(&mut *out, "storage", |out| {
            match read_field_str(particle, "_class") {
                // A class this module does not handle answers null and does
                // not end the program: it may have been meant for something
                // else entirely.
                Some("Get") | Some("Set") | Some("Remove") => exception(out, "storage", NO_BROWSER),
                _ => null(out),
            }
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod page {
    include!("../../browser_half.rs");

    browser_half!(
        "storage",
        storage_code_module_abi_version,
        storage_code_module_dispatch
    );
}

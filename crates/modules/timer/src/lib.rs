//! The `timer` native module — a particle, later.
//!
//! Handlers:
//!
//! - `Delay { ms, then }` — answers `DelayResult { value }`, the number this
//!   delay is known by. After `ms` milliseconds, `then` arrives: a whole
//!   particle, written where the delay is asked for, or just a class name
//!   when there is nothing else to say.
//! - `Cancel { id }` — answers `CancelResult { ok }`. Cancelling one that
//!   has already fired, or was never started, is `ok = false` rather than a
//!   failure: it means the same thing either way.
//!
//! ```text
//! emit Delay { ms = 5000, then = { _class = "Refresh", what = "prices" } } to timer get d
//!
//! Refresh { what } => {
//!     ...and re-arm, if it should keep going
//! }
//! ```
//!
//! The application names the particle it wants back, in advance — the same
//! rule `dom` follows for a click and `router` for a path. It is not a
//! boundary; see `router`'s file for why there cannot be one between a page
//! and the module it loaded.
//!
//! **Nothing repeats on its own.** A delay fires once; a handler that wants
//! a heartbeat asks for the next one itself. Repeating would mean a timer
//! outliving the reason it was started, which is how a program ends up doing
//! work nobody asked for — and re-arming is one line at the end of the
//! handler that already ran.
//!
//! # It does not hold the program open
//!
//! On a machine a program ends at its last statement unless a module says it
//! is still serving (`code_abi.h` item 8). A pending delay does not say
//! that: an application that wants to stay up is serving something — a
//! socket, a queue — and a timer is not that thing. So a program whose only
//! module is this one ends, with its delay unfired, which is what it asked
//! for by having nothing else to do.
//!
//! # Where it works
//!
//! **A browser today.** On a machine every handler answers an `Exception`
//! saying so; the thread-and-queue half is written down as the next step and
//! not yet built, since what needed a timer first was a page.
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
    //! Not built yet, and it says so rather than accepting a delay it will
    //! never fire. A machine can have this — a thread and the inbound queue
    //! (`code_abi.h` item 6) are exactly what it would take — but nothing
    //! has needed it, and a module that silently drops what it was asked to
    //! remember is worse than one that has not been written.
    use code_native::*;

    const NOT_YET: &str =
        "`timer` runs in a browser today; the machine half is not built yet, and it will not \
         accept a delay it cannot fire. Ask `Linked` to find out which you are";

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
        guarded(&mut *out, "timer", |out| {
            match read_field_str(particle, "_class") {
                // A class this module does not handle answers null and does
                // not end the program: it may have been meant for something
                // else entirely.
                Some("Delay") | Some("Cancel") => exception(out, "timer", NOT_YET),
                _ => null(out),
            }
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod page {
    include!("../../browser_half.rs");

    browser_half!(
        "timer",
        timer_code_module_abi_version,
        timer_code_module_dispatch
    );
}

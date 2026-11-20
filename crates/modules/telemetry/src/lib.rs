//! The `telemetry` native module — what went wrong on a page, kept and sent.
//!
//! Handlers:
//!
//! - `Configure { url, token, app? }` — where the page's trouble goes (a
//!   same-origin path) and as whom; a null token stops sending and keeps
//!   what is there. Answers `ConfigureResult { ok }`.
//! - `Note { level, message, kind? }` — an application's own word: `warn`
//!   or `error` is a line of trouble, anything else a step on the trail.
//!   Answers `NoteResult { ok }`.
//! - `Flush` — send what is kept now. Answers `FlushResult { ok }`.
//! - `Kept` — what is waiting, oldest first: `KeptResult { items }`.
//!
//! Without being asked it also keeps script errors nothing caught, module
//! halves that threw (the runtime), and requests that never came back as a
//! particle (`net_client`), each with the last steps before it. One
//! collector per page, shared by a shell and every guest it runs.
//!
//! # Where it works
//!
//! **A browser.** On a machine every handler answers an `Exception` saying
//! so: a held program there already has `Log` through its membrane, which
//! the host keeps. The module is still linkable, so one application can be
//! built both ways.
//!
//! Its page half is in `page.mjs`, with every other browser module's — see
//! `crates/modules/browser_half.rs` for the wasm half, written once.

#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
    //! Where there is no page, and so no page's trouble to keep.
    use code_native::*;

    const NO_BROWSER: &str =
        "there is no browser here — `telemetry` keeps a page's trouble, and this program is \
         running on a machine, where `Log` to the membrane is what the host keeps";

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
        guarded(&mut *out, "telemetry", |out| match read_field_str(particle, "_class") {
            Some("Configure") | Some("Note") | Some("Flush") | Some("Kept") => {
                exception(out, "telemetry", NO_BROWSER)
            }
            // A class this module does not handle answers null.
            _ => null(out),
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod page {
    include!("../../browser_half.rs");

    browser_half!(
        "telemetry",
        telemetry_code_module_abi_version,
        telemetry_code_module_dispatch
    );
}

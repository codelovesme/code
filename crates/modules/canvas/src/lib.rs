//! The `canvas` module — draw bounded 2D command frames into a browser page.
//!
//! `Draw { into?, width, height, commands, event? } → CanvasResult { ok,
//! width, height, view_width, view_height, time, reason? }` clears the
//! selected canvas and paints the frame, answering the element's on-page size
//! and the page clock so a scene can fit the screen and move by time.
//! An optional event class receives `{ x, y }` on a pointer click, with the
//! coordinates expressed in the frame's logical width and height. The page
//! resizes the backing surface for its display density while preserving those
//! logical coordinates.
//!
//! The page half interprets data commands only; it does not evaluate code or
//! accept raw HTML. See `README.md` for the command vocabulary and limits.

#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
    use code_native::*;

    const NO_BROWSER: &str =
        "there is no browser here — `canvas` draws into a page, and this program is running on a machine; ask `Linked` which build target is active";

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
        guarded(&mut *out, "canvas", |out| {
            match read_field_str(particle, "_class") {
                Some("Draw") | Some("Keep") => exception(out, "canvas", NO_BROWSER),
                _ => null(out),
            }
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod page {
    include!("../../browser_half.rs");

    browser_half!("canvas", canvas_code_module_abi_version, canvas_code_module_dispatch);
}

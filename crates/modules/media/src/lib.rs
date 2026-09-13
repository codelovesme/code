//! The `media` native module — the page's microphone and camera.
//!
//! Handlers:
//!
//! - `Record {}` — start recording audio. Answers `RecordResult { ok }` at
//!   once; the recording itself arrives later as a `Recorded` particle.
//! - `StopRecording {}` — stop, and fire `Recorded { audio_base64, format,
//!   ms }`. Answers `StopResult { ok }`.
//! - `StartCamera {}` — open the camera and fill any viewfinder in the page
//!   (see below). Answers `StartCameraResult { ok }`.
//! - `TakePhoto {}` — grab the current frame and fire `Captured {
//!   image_base64, width, height }`. Answers `TakePhotoResult { ok }`.
//! - `StopCamera {}` — release the camera. Answers `StopResult { ok }`.
//!
//! Asking for a device is asking a *person*, and they may say no. A refusal
//! fires `Denied { device, reason }` rather than an `Exception`: a reader
//! declining the microphone is an answer, not a fault. A browser without the
//! device at all fires `Unavailable { device, reason }`.
//!
//! # Why the answer comes later
//!
//! Nothing here can hand back a recording as the answer to `Record`: the
//! recording does not exist yet, and waiting means blocking the page. So the
//! handler answers "started", and the bytes arrive as their own particle,
//! the same shape `net_client` uses for a reply that outlives the request.
//!
//! # The viewfinder
//!
//! A camera needs somewhere to show itself, and `dom` is what draws — it
//! renders a tree that is *data*, with no raw HTML and no property
//! assignment, so nothing an application writes can hand an element a
//! `MediaStream`.
//!
//! So the application marks a node and this module fills it:
//!
//! ```code
//! { tag = "video", media = "camera", attrs = { autoplay = "autoplay" } }
//! ```
//!
//! `dom` turns `media = "camera"` into a plain attribute and does nothing
//! else with it; this module watches the page for that attribute and
//! attaches the stream wherever it appears. The tree stays data, `dom` keeps
//! its one rule, and a redraw — which replaces the element — is picked up
//! again rather than losing the picture.
//!
//! # Where it works
//!
//! **A browser.** On a machine every handler answers an `Exception` saying
//! so: a microphone belongs to whoever is sitting there, and a program with
//! nobody sitting there has no business inventing one.
//!
//! For wasm it is built as an archive linked into the program:
//!
//! ```bash
//! cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
//! ```
//!
//! Its page half is in `page.mjs`, with every other browser module's.

#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
    //! Where there is no browser, and so no one to ask.
    use code_native::*;

    const NO_BROWSER: &str =
        "there is no browser here — `media` is the page's microphone and camera, and this \
         program is running on a machine. A device belongs to whoever is sitting in front of \
         it; ask `Linked` to find out which you are";

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
        guarded(&mut *out, "media", |out| {
            match read_field_str(particle, "_class") {
                Some("Record") | Some("StopRecording") | Some("StartCamera")
                | Some("TakePhoto") | Some("StopCamera") => exception(out, "media", NO_BROWSER),
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
        "media",
        media_code_module_abi_version,
        media_code_module_dispatch
    );
}

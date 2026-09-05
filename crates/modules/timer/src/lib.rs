//! The `timer` native module — a particle, later.
//!
//! Handlers:
//!
//! - `Delay { ms, then, value? }` — answers `DelayResult { value }`, the
//!   number this delay is known by. After `ms` milliseconds a particle of
//!   class `then` arrives, carrying `value` if one was given.
//! - `Cancel { id }` — answers `CancelResult { ok }`. Cancelling one that
//!   has already fired, or was never started, is `ok = false` rather than a
//!   failure: it means the same thing either way.
//!
//! ```text
//! emit Delay { ms = 5000, then = "Refresh", value = "prices" } to timer get d
//!
//! Refresh { value } => {
//!     ...and re-arm, if it should keep going
//! }
//! ```
//!
//! The application names the class it wants back, in advance — the same rule
//! `dom` follows for a click and `router` for a path. What it buys is a fixed
//! shape: what arrives is a class and at most one piece of text. It is not a
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
//! Two functions: `code_web_timer_set` and `code_web_timer_clear`.
//! `web/host.mjs` in this repository supplies both, for every browser module
//! at once.
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
    //! The half that can actually wait. Hand-rolled against `code_abi.h`.
    use core::ffi::{c_char, c_void, CStr};

    /// `CODE_VALUE_SLOT_SIZE`.
    const SLOT: usize = 80;
    const NUMBER: i32 = 0;
    const STR: i32 = 1;
    const NULL: i32 = 3;
    const OBJECT: i32 = 5;

    #[repr(C)]
    pub struct CodeValue {
        pub tag: i32,
        pub heap: i32,
        pub number: f64,
        pub str_: *const c_char,
        pub boolean: i32,
        pub items: *mut c_void,
        pub keys: *const *const c_char,
        pub len: i64,
    }

    // The page's half, and the only thing this module can reach. `set`
    // answers the number the delay is known by, or a negative one when it
    // could not be started; `clear` answers non-zero when there was still
    // one to cancel.
    extern "C" {
        fn code_web_timer_set(
            ms: f64,
            class: *const u8,
            class_len: usize,
            value: *const u8,
            value_len: i32,
        ) -> i32;
        fn code_web_timer_clear(id: i32) -> i32;
        fn code_str(out: *mut CodeValue, s: *const c_char);
        fn code_number(out: *mut CodeValue, n: f64);
        fn code_bool(out: *mut CodeValue, b: i32);
        fn code_object(
            out: *mut CodeValue,
            keys: *const *const c_char,
            values: *mut c_void,
            len: i64,
        );
        fn code_release(v: *mut CodeValue);
    }

    unsafe fn slot(v: &CodeValue, i: i64) -> &CodeValue {
        &*((v.items as *const u8).add(i as usize * SLOT) as *const CodeValue)
    }

    /// Fields are read by walking `keys`/`items` — `code_abi.h` says so.
    unsafe fn field<'a>(v: &'a CodeValue, name: &str) -> Option<&'a CodeValue> {
        if v.tag != OBJECT || v.keys.is_null() || v.items.is_null() {
            return None;
        }
        for i in 0..v.len {
            if CStr::from_ptr(*v.keys.offset(i as isize)).to_bytes() == name.as_bytes() {
                return Some(slot(v, i));
            }
        }
        None
    }

    unsafe fn text_of(v: &CodeValue) -> Option<&[u8]> {
        if v.tag == STR && !v.str_.is_null() {
            Some(CStr::from_ptr(v.str_).to_bytes())
        } else {
            None
        }
    }

    /// `{ _class = <class>, ok = <ok> }`.
    unsafe fn answer_ok(out: *mut CodeValue, class: &CStr, ok: bool) {
        let mut slots = [0u8; 2 * SLOT];
        let keys: [*const c_char; 2] = [c"_class".as_ptr(), c"ok".as_ptr()];
        code_str(slots.as_mut_ptr() as *mut CodeValue, class.as_ptr());
        code_bool(slots.as_mut_ptr().add(SLOT) as *mut CodeValue, ok as i32);
        code_object(out, keys.as_ptr(), slots.as_mut_ptr() as *mut c_void, 2);
        code_release(slots.as_mut_ptr() as *mut CodeValue);
        code_release(slots.as_mut_ptr().add(SLOT) as *mut CodeValue);
    }

    /// `{ _class = <class>, value = <number or null> }` — the number a delay
    /// is known by, or null when it could not be started.
    unsafe fn answer_id(out: *mut CodeValue, class: &CStr, id: Option<i32>) {
        let mut slots = [0u8; 2 * SLOT];
        let keys: [*const c_char; 2] = [c"_class".as_ptr(), c"value".as_ptr()];
        code_str(slots.as_mut_ptr() as *mut CodeValue, class.as_ptr());
        let value = slots.as_mut_ptr().add(SLOT) as *mut CodeValue;
        match id {
            Some(id) => code_number(value, id as f64),
            None => (*value).tag = NULL,
        }
        code_object(out, keys.as_ptr(), slots.as_mut_ptr() as *mut c_void, 2);
        code_release(slots.as_mut_ptr() as *mut CodeValue);
        code_release(value);
    }

    unsafe fn null(out: *mut CodeValue) {
        let mut value = CodeValue {
            tag: NULL,
            heap: 0,
            number: 0.0,
            str_: core::ptr::null(),
            boolean: 0,
            items: core::ptr::null_mut(),
            keys: core::ptr::null(),
            len: 0,
        };
        core::ptr::copy_nonoverlapping(&mut value as *mut CodeValue, out, 1);
    }

    #[no_mangle]
    pub extern "C" fn timer_code_module_abi_version() -> u32 {
        1
    }

    /// # Safety
    ///
    /// Both pointers must be valid for the duration of the call and laid out
    /// per `code_abi.h` — the host guarantees this on every dispatch.
    #[no_mangle]
    pub unsafe extern "C" fn timer_code_module_dispatch(
        out: *mut CodeValue,
        particle: *const CodeValue,
    ) {
        let particle = &*particle;
        let class = field(particle, "_class").and_then(|c| text_of(c));

        match class {
            Some(b"Delay") => {
                let then = field(particle, "then").and_then(|t| text_of(t));
                let Some(then) = then.filter(|t| !t.is_empty()) else {
                    // Nothing to fire. A delay whose particle nobody named
                    // would spend the wait and then have nowhere to go.
                    answer_id(out, c"DelayResult", None);
                    return;
                };
                let ms = match field(particle, "ms") {
                    Some(v) if v.tag == NUMBER => v.number.max(0.0),
                    // No `ms` means as soon as the program is between
                    // statements again — the shortest wait there is, and a
                    // useful one for handing work back to the page.
                    _ => 0.0,
                };
                // Optional, and a negative length says there is none: a
                // delay carrying nothing makes a particle with no `value`
                // field rather than one holding "".
                let (value_ptr, value_len) = match field(particle, "value").and_then(|v| text_of(v))
                {
                    Some(value) => (value.as_ptr(), value.len() as i32),
                    None => (core::ptr::null(), -1),
                };
                let id = code_web_timer_set(ms, then.as_ptr(), then.len(), value_ptr, value_len);
                answer_id(out, c"DelayResult", (id >= 0).then_some(id));
            }
            Some(b"Cancel") => match field(particle, "id") {
                Some(v) if v.tag == NUMBER => {
                    let ok = code_web_timer_clear(v.number as i32);
                    answer_ok(out, c"CancelResult", ok != 0);
                }
                // Cancelling something this module cannot name is false,
                // like cancelling one that has already fired.
                _ => answer_ok(out, c"CancelResult", false),
            },
            // A class this module does not handle is null, not an error.
            _ => null(out),
        }
    }

    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo) -> ! {
        core::arch::wasm32::unreachable()
    }
}

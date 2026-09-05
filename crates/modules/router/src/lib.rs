//! The `router` native module — where in the application the reader is.
//!
//! Handlers:
//!
//! - `Route {}` — answers `RouteResult { value }`, the path shown now.
//! - `Navigate { path }` — goes there, and answers `NavigateResult { ok }`.
//!   The page's history gains an entry, so Back means what a reader expects.
//! - `Watch { then }` — answers `WatchResult { ok }`, and from then on every
//!   change of the path — a link, Back, Forward, an address typed by hand —
//!   arrives as a particle of class `then`, carrying the new path as
//!   `path`.
//!
//! # Watching is the whole of it
//!
//! ```text
//! emit Watch { then = "Went" } to router get w
//!
//! Went { path } => {
//!     ...draw the page for `path`
//! }
//! ```
//!
//! The application names the class it wants back, in advance — the same rule
//! `dom` follows for a click. What that buys is a fixed *shape*: what arrives
//! is always a class and at most one piece of text, so a page cannot invent a
//! particle with fields of its own choosing.
//!
//! It is not a boundary, and it is worth saying so plainly. A page and the
//! module it loaded share one memory, and nothing stops a page from naming a
//! class the application never offered it. Anything that could is already
//! able to write to that memory directly. What actually protects an
//! application from the page it runs in is on the other side of the network,
//! where the two really are separate.
//!
//! `Navigate` fires it too. An application that draws in one place — the
//! handler — does not then have to draw again at every call site, and the
//! two ways a path can change stop being two paths through the code.
//!
//! # Where it works
//!
//! **A browser.** On a machine every handler answers an `Exception` saying
//! so. The module is still linkable there, so one application can be built
//! both ways and ask [`Linked`](../../../README.md#linked) which it is.
//!
//! For wasm it is built as an archive linked into the program:
//!
//! ```bash
//! cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
//! ```
//!
//! # What the page has to supply
//!
//! Three functions: `code_web_route_get`, `code_web_route_set` and
//! `code_web_route_watch`. `web/host.mjs` in this repository supplies all of
//! them, for every browser module at once.
//!
//! Its wasm half is `no_std` and hand-written against `code_abi.h` — see
//! `dom`'s file for why a module meant for the browser brings no standard
//! library.

#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
    //! Where there is no address bar. Every handler answers an `Exception`
    //! saying so rather than inventing a path: a program that thought it
    //! knew where it was would draw the wrong page and never find out why.
    use code_native::*;

    const NO_BROWSER: &str =
        "there is no address bar here — `router` is where a page is, and this program is \
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
        guarded(&mut *out, "router", |out| {
            match read_field_str(particle, "_class") {
                // A class this module does not handle answers null and does
                // not end the program: it may have been meant for something
                // else entirely.
                Some("Route") | Some("Navigate") | Some("Watch") => {
                    exception(out, "router", NO_BROWSER)
                }
                _ => null(out),
            }
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod page {
    //! The half that can actually move. Hand-rolled against `code_abi.h`.
    use core::ffi::{c_char, c_void, CStr};

    /// `CODE_VALUE_SLOT_SIZE`.
    const SLOT: usize = 80;
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

    // The page's half, and the only thing this module can reach. `get`
    // answers how many bytes it wrote; `set` and `watch` answer non-zero
    // when they took.
    extern "C" {
        fn code_web_route_get(out: *mut u8, cap: usize) -> i32;
        fn code_web_route_set(path: *const u8, path_len: usize) -> i32;
        fn code_web_route_watch(class: *const u8, class_len: usize) -> i32;
        fn code_str(out: *mut CodeValue, s: *const c_char);
        fn code_str_owned(out: *mut CodeValue, s: *const c_char);
        fn code_bool(out: *mut CodeValue, b: i32);
        fn code_object(
            out: *mut CodeValue,
            keys: *const *const c_char,
            values: *mut c_void,
            len: i64,
        );
        fn code_release(v: *mut CodeValue);
    }

    /// Where the page writes the path when asked for it.
    ///
    /// A buffer of ours rather than an address of the page's, so this module
    /// never trusts an address or a length that came from outside: the read
    /// stays inside its own array, bounded by a capacity it set. Containment
    /// rather than protection — the page can reach this memory whichever way
    /// it is asked to. Longer than any path a browser will carry, and a
    /// longer one is cut rather than refused.
    const CAP: usize = 4096;
    static mut PATH: [u8; CAP + 1] = [0; CAP + 1];

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

    /// `{ _class = <class>, value = <path> }`. Copied, since `PATH` is
    /// refilled by the next read.
    unsafe fn answer_path(out: *mut CodeValue, class: &CStr, path: &CStr) {
        let mut slots = [0u8; 2 * SLOT];
        let keys: [*const c_char; 2] = [c"_class".as_ptr(), c"value".as_ptr()];
        code_str(slots.as_mut_ptr() as *mut CodeValue, class.as_ptr());
        code_str_owned(
            slots.as_mut_ptr().add(SLOT) as *mut CodeValue,
            path.as_ptr(),
        );
        code_object(out, keys.as_ptr(), slots.as_mut_ptr() as *mut c_void, 2);
        code_release(slots.as_mut_ptr() as *mut CodeValue);
        code_release(slots.as_mut_ptr().add(SLOT) as *mut CodeValue);
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
    pub extern "C" fn router_code_module_abi_version() -> u32 {
        1
    }

    /// # Safety
    ///
    /// Both pointers must be valid for the duration of the call and laid out
    /// per `code_abi.h` — the host guarantees this on every dispatch.
    #[no_mangle]
    pub unsafe extern "C" fn router_code_module_dispatch(
        out: *mut CodeValue,
        particle: *const CodeValue,
    ) {
        let particle = &*particle;
        let class = field(particle, "_class").and_then(|c| text_of(c));

        match class {
            Some(b"Route") => {
                let read = code_web_route_get(core::ptr::addr_of_mut!(PATH) as *mut u8, CAP);
                let read = if read < 0 {
                    0
                } else {
                    (read as usize).min(CAP)
                };
                let buf = &mut *core::ptr::addr_of_mut!(PATH);
                buf[read] = 0;
                answer_path(
                    out,
                    c"RouteResult",
                    CStr::from_ptr(buf.as_ptr() as *const c_char),
                );
            }
            Some(b"Navigate") => match field(particle, "path").and_then(|p| text_of(p)) {
                Some(path) => {
                    let ok = code_web_route_set(path.as_ptr(), path.len());
                    answer_ok(out, c"NavigateResult", ok != 0);
                }
                // A path this module cannot read is a mistake worth an
                // answer, not a silent stay-where-you-are.
                None => answer_ok(out, c"NavigateResult", false),
            },
            Some(b"Watch") => match field(particle, "then").and_then(|t| text_of(t)) {
                Some(class) if !class.is_empty() => {
                    let ok = code_web_route_watch(class.as_ptr(), class.len());
                    answer_ok(out, c"WatchResult", ok != 0);
                }
                // Without a class there is nothing to fire, and a watch that
                // fires nothing is worse than one that says it did not start.
                _ => answer_ok(out, c"WatchResult", false),
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

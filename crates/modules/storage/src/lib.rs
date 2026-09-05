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
//! Three functions, and they are the only things this module can reach:
//! `code_web_storage_get`, `code_web_storage_set` and
//! `code_web_storage_remove`. `web/host.mjs` in this repository supplies all
//! of them, for every browser module at once.
//!
//! The page never chooses where to write in the program's memory: reading a
//! value means writing into a buffer of this module's, whose address and
//! capacity it is given. That is the same rule the event path follows, for
//! the same reason.
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
    //! The half that can actually remember. Hand-rolled against
    //! `code_abi.h`.
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
    // answers how many bytes it wrote, or a negative number when the key
    // holds nothing; `set` answers non-zero when the browser accepted it.
    extern "C" {
        fn code_web_storage_get(key: *const u8, key_len: usize, out: *mut u8, cap: usize) -> i32;
        fn code_web_storage_set(
            key: *const u8,
            key_len: usize,
            value: *const u8,
            value_len: usize,
        ) -> i32;
        fn code_web_storage_remove(key: *const u8, key_len: usize) -> i32;
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

    /// Where the page writes a value it was asked for.
    ///
    /// A buffer of ours rather than an address of the page's: a page choosing
    /// where to write in this program's memory could write anywhere. One
    /// buffer, refilled per read, because a read has finished before the next
    /// can start. A stored value longer than this is answered as far as it
    /// fits rather than refused — the bound is generous, and half a
    /// remembered string is better than a program that cannot start.
    const CAP: usize = 64 * 1024;
    static mut READ: [u8; CAP + 1] = [0; CAP + 1];

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

    /// `{ _class = <class>, value = <text or null> }`.
    ///
    /// The text is *copied* into the value: `READ` is refilled by the next
    /// read, while whatever the program does with the answer is its own
    /// business and may outlast that.
    unsafe fn answer_value(out: *mut CodeValue, class: &CStr, text: Option<&CStr>) {
        let mut slots = [0u8; 2 * SLOT];
        let keys: [*const c_char; 2] = [c"_class".as_ptr(), c"value".as_ptr()];
        code_str(slots.as_mut_ptr() as *mut CodeValue, class.as_ptr());
        let value = slots.as_mut_ptr().add(SLOT) as *mut CodeValue;
        match text {
            Some(text) => code_str_owned(value, text.as_ptr()),
            None => (*value).tag = NULL,
        }
        code_object(out, keys.as_ptr(), slots.as_mut_ptr() as *mut c_void, 2);
        code_release(slots.as_mut_ptr() as *mut CodeValue);
        code_release(value);
    }

    /// The one field every handler here needs. A particle without it, or
    /// with a key that is not text, is answered as a failure rather than
    /// looked up under the empty string.
    unsafe fn key_of(particle: &CodeValue) -> Option<&[u8]> {
        field(particle, "key").and_then(|k| text_of(k))
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
    pub extern "C" fn storage_code_module_abi_version() -> u32 {
        1
    }

    /// # Safety
    ///
    /// Both pointers must be valid for the duration of the call and laid out
    /// per `code_abi.h` — the host guarantees this on every dispatch.
    #[no_mangle]
    pub unsafe extern "C" fn storage_code_module_dispatch(
        out: *mut CodeValue,
        particle: *const CodeValue,
    ) {
        let particle = &*particle;
        let class = field(particle, "_class").and_then(|c| text_of(c));

        match class {
            Some(b"Get") => {
                let Some(key) = key_of(particle) else {
                    answer_value(out, c"GetResult", None);
                    return;
                };
                let read = code_web_storage_get(
                    key.as_ptr(),
                    key.len(),
                    core::ptr::addr_of_mut!(READ) as *mut u8,
                    CAP,
                );
                if read < 0 {
                    // Nothing stored there. Null rather than "" — a key that
                    // was never set and one holding an empty string are
                    // different answers.
                    answer_value(out, c"GetResult", None);
                    return;
                }
                let read = (read as usize).min(CAP);
                let buf = &mut *core::ptr::addr_of_mut!(READ);
                buf[read] = 0;
                answer_value(
                    out,
                    c"GetResult",
                    Some(CStr::from_ptr(buf.as_ptr() as *const c_char)),
                );
            }
            Some(b"Set") => {
                let value = field(particle, "value").and_then(|v| text_of(v));
                match (key_of(particle), value) {
                    (Some(key), Some(value)) => {
                        let ok = code_web_storage_set(
                            key.as_ptr(),
                            key.len(),
                            value.as_ptr(),
                            value.len(),
                        );
                        answer_ok(out, c"SetResult", ok != 0);
                    }
                    // Only text is stored, so a number or an object is not
                    // quietly rendered into one: the application says what
                    // the text is, with `json` if that is what it means.
                    _ => answer_ok(out, c"SetResult", false),
                }
            }
            Some(b"Remove") => match key_of(particle) {
                Some(key) => {
                    let ok = code_web_storage_remove(key.as_ptr(), key.len());
                    answer_ok(out, c"RemoveResult", ok != 0);
                }
                None => answer_ok(out, c"RemoveResult", false),
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

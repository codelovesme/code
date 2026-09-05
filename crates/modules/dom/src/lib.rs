//! The `dom` native module — putting a tree of nodes on the page.
//!
//! Handlers:
//!
//! - `Render { into?, styles?, tree }` — replaces the contents of `into` (a
//!   CSS selector, `"body"` by default) with `tree`, sets `styles` as the
//!   page's stylesheet, and answers `RenderResult { ok }`. `ok` is false when
//!   the selector matched nothing.
//!
//! # The tree is a value, not markup
//!
//! A node is `{ tag, attrs?, children? }` and a string is a text node:
//!
//! ```text
//! { tag = "ul", children = [
//!     { tag = "li", attrs = { class = "row" }, children = ["kahve"] }
//! ] }
//! ```
//!
//! That is the whole vocabulary. There is **no raw HTML, no event handler and
//! no property assignment** — a tree can describe elements, attributes and
//! text, and nothing else. So a tree built out of someone's name or a
//! translated string is data all the way to the page and cannot become code
//! on the way. The page's half is held to the same rule.
//!
//! `tree` may also be a **string**, which is taken as JSON that is already in
//! this shape and passed through untouched. That is for an application that
//! built the text itself; the ordinary case is to hand over a value and let
//! this module serialise it.
//!
//! # Where appearance lives
//!
//! In the same particle, and **not on the nodes**:
//!
//! ```text
//! emit Render {
//!     styles = {
//!         ".cart"  = { "max-width" = "24rem", padding = "1rem" },
//!         ".total" = { "font-weight" = "600" }
//!     },
//!     tree = { tag = "p", attrs = { class = "total" }, children = ["57 TL"] }
//! } to dom
//! ```
//!
//! A node says what it *is* — `class = "total"` — and the rules say what that
//! looks like, once, in one place. Both travel in the same JSON, so there is
//! no stylesheet file to keep in step with the application and nothing to
//! serve beside it.
//!
//! That split is the whole point. Colours and positions written onto every
//! node would make the code that builds a page *be* the page's design, which
//! is exactly what a gene should not turn into. A genuinely per-node value —
//! a bar's width computed from data — is an ordinary attribute
//! (`attrs = { style = "width: 40%" }`) and needs nothing from this module.
//!
//! `styles` is a value, not CSS text: selector to properties to values. So
//! there is no stylesheet to parse, and nothing that could end a rule early
//! and start a different one.
//!
//! # Where it works
//!
//! Only in a wasm build, because a page is the only thing this module talks
//! to. On a machine every handler answers an `Exception` saying so, rather
//! than pretending to draw — the module is still linkable there so that one
//! application can be built for both and find out at runtime (`Linked`)
//! which it is.
//!
//! The page supplies one function, and it is the only thing this module can
//! reach: it takes the JSON and the selector, and answers whether the
//! selector matched.
//!
//! # Why the wasm half is written out by hand
//!
//! It is `no_std`, and it does not use `code-native`, which the native half
//! does. Not a style choice — **two `code-native` modules cannot be linked
//! into one `.wasm` at all.** Each brings its own copy of Rust's standard
//! library, so its private symbols (`rust_eh_personality`, the panic
//! machinery) end up defined twice, and the wasm linker has no flag to
//! forgive that the way a native one does. A web application links several
//! modules by definition, so the module that is *for* the web is the one
//! that has to bring nothing.
//!
//! It is also most of the size. Measured on one small page, `no_std` costs
//! about 25 KB against 245 KB with the standard library — and that is paid
//! per module, since the duplicate copies are exactly the problem above.
//!
//! The lasting fix is a `no_std` mode for `code-native` itself, at which
//! point this file collapses back into one implementation. Until then the
//! rule is: **a module meant for wasm brings no standard library.**

#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
    //! Where there is no page. Every handler answers an `Exception` saying
    //! so, rather than pretending to draw — an application drawing into
    //! nothing is a mistake worth naming, not a page that happened to be
    //! empty. The module is still linkable here so one application can be
    //! built for both and ask `Linked` which it is.
    use code_native::*;

    const NO_PAGE: &str =
        "there is no page here — `dom` draws in a browser, and this program is running on a \
         machine. Ask `Linked` to find out which you are";

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
        guarded(&mut *out, "dom", |out| {
            match read_field_str(particle, "_class") {
                // A class this module does not handle answers null and does
                // not end the program: it may have been meant for something
                // else entirely.
                Some("Render") => exception(out, "dom", NO_PAGE),
                _ => null(out),
            }
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod page {
    //! The half that can actually draw. Hand-rolled against `code_abi.h`
    //! rather than built on `code-native` — see the note at the top of this
    //! file for why that is forced rather than chosen.
    use core::ffi::{c_char, c_void, CStr};

    /// `CODE_VALUE_SLOT_SIZE`.
    const SLOT: usize = 80;
    const NUMBER: i32 = 0;
    const STR: i32 = 1;
    const BOOL: i32 = 2;
    const NULL: i32 = 3;
    const ARRAY: i32 = 4;
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

    // The page's half, and the only thing this module can reach. It cannot
    // fail in a way this module could act on; it answers non-zero when the
    // selector matched.
    extern "C" {
        fn code_web_render(json: *const u8, json_len: usize, into: *const u8, into_len: usize) -> i32;
        fn code_str(out: *mut CodeValue, s: *const c_char);
        fn code_bool(out: *mut CodeValue, b: i32);
        fn code_object(out: *mut CodeValue, keys: *const *const c_char, values: *mut c_void, len: i64);
        fn code_release(v: *mut CodeValue);
    }

    /// A fixed buffer rather than an allocator: `no_std` means bringing one
    /// of our own, and a page that wants to draw more than this at once can
    /// draw it in pieces. Overflow is refused rather than truncated — half a
    /// tree is not a smaller tree.
    const CAP: usize = 256 * 1024;

    struct Buf {
        bytes: [u8; CAP],
        len: usize,
        full: bool,
    }

    impl Buf {
        fn push(&mut self, s: &[u8]) {
            if self.full || self.len + s.len() > CAP {
                self.full = true;
                return;
            }
            self.bytes[self.len..self.len + s.len()].copy_from_slice(s);
            self.len += s.len();
        }

        fn byte(&mut self, b: u8) {
            self.push(&[b]);
        }

        /// JSON string escaping. `<` and `>` go out as escapes too — which
        /// JSON allows — so a serialised tree can never close a tag even if
        /// something later puts the text somewhere it should not be.
        fn string(&mut self, s: &[u8]) {
            self.byte(b'"');
            for &c in s {
                match c {
                    b'"' => self.push(b"\\\""),
                    b'\\' => self.push(b"\\\\"),
                    b'\n' => self.push(b"\\n"),
                    b'\r' => self.push(b"\\r"),
                    b'\t' => self.push(b"\\t"),
                    b'<' => self.push(b"\\u003c"),
                    b'>' => self.push(b"\\u003e"),
                    0x00..=0x1f => {
                        const HEX: &[u8; 16] = b"0123456789abcdef";
                        self.push(b"\\u00");
                        self.byte(HEX[(c >> 4) as usize]);
                        self.byte(HEX[(c & 15) as usize]);
                    }
                    _ => self.byte(c),
                }
            }
            self.byte(b'"');
        }

        /// Whole numbers, spelled by hand. A fractional attribute has no
        /// meaning in a tree, and an application that wants one formats it
        /// into a string itself — where the language's own number-to-text
        /// does the work properly.
        fn number(&mut self, n: f64) {
            let mut n = n;
            if n < 0.0 {
                self.byte(b'-');
                n = -n;
            }
            let mut i = n as u64;
            let mut digits = [0u8; 20];
            let mut d = 0;
            loop {
                digits[d] = b'0' + (i % 10) as u8;
                i /= 10;
                d += 1;
                if i == 0 {
                    break;
                }
            }
            while d > 0 {
                d -= 1;
                self.byte(digits[d]);
            }
        }
    }

    unsafe fn slot(v: &CodeValue, i: i64) -> &CodeValue {
        &*((v.items as *const u8).add(i as usize * SLOT) as *const CodeValue)
    }

    /// Fields are read by walking `keys`/`items` — `code_abi.h` says so, and
    /// removed the accessors that looked like they would do it for you.
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

    unsafe fn write_json(b: &mut Buf, v: &CodeValue) {
        match v.tag {
            STR => b.string(text_of(v).unwrap_or(b"")),
            NUMBER => b.number(v.number),
            BOOL => b.push(if v.boolean != 0 { b"true" } else { b"false" }),
            NULL => b.push(b"null"),
            ARRAY => {
                b.byte(b'[');
                for i in 0..v.len {
                    if i > 0 {
                        b.byte(b',');
                    }
                    write_json(b, slot(v, i));
                }
                b.byte(b']');
            }
            OBJECT => {
                b.byte(b'{');
                for i in 0..v.len {
                    if i > 0 {
                        b.byte(b',');
                    }
                    b.string(CStr::from_ptr(*v.keys.offset(i as isize)).to_bytes());
                    b.byte(b':');
                    write_json(b, slot(v, i));
                }
                b.byte(b'}');
            }
            _ => b.push(b"null"),
        }
    }

    /// `{ _class = <class>, ok = <ok> }`.
    unsafe fn answer(out: *mut CodeValue, class: &CStr, ok: bool) {
        let mut slots = [0u8; 2 * SLOT];
        let keys: [*const c_char; 2] = [c"_class".as_ptr(), c"ok".as_ptr()];
        code_str(slots.as_mut_ptr() as *mut CodeValue, class.as_ptr());
        code_bool(slots.as_mut_ptr().add(SLOT) as *mut CodeValue, ok as i32);
        code_object(out, keys.as_ptr(), slots.as_mut_ptr() as *mut c_void, 2);
        code_release(slots.as_mut_ptr() as *mut CodeValue);
        code_release(slots.as_mut_ptr().add(SLOT) as *mut CodeValue);
    }

    #[no_mangle]
    pub extern "C" fn dom_code_module_abi_version() -> u32 {
        1
    }

    /// # Safety
    ///
    /// Both pointers must be valid for the duration of the call and laid out
    /// per `code_abi.h` — the host guarantees this on every dispatch.
    #[no_mangle]
    pub unsafe extern "C" fn dom_code_module_dispatch(
        out: *mut CodeValue,
        particle: *const CodeValue,
    ) {
        let particle = &*particle;
        let class = field(particle, "_class").and_then(|c| text_of(c));

        if class == Some(b"Render".as_ref()) {
            let mut ok = false;
            if let Some(tree) = field(particle, "tree") {
                let into = field(particle, "into")
                    .and_then(|v| text_of(v))
                    .unwrap_or(b"body");
                let mut buf = Buf { bytes: [0; CAP], len: 0, full: false };

                // One payload carrying both halves — the rules and the tree
                // — so a page needs nothing beside the application.
                buf.push(b"{\"styles\":");
                match field(particle, "styles") {
                    Some(styles) => write_json(&mut buf, styles),
                    None => buf.push(b"null"),
                }
                buf.push(b",\"tree\":");
                match text_of(tree) {
                    // Already JSON in this shape — passed through, so an
                    // application that built the text itself is not made to
                    // pay for a second pass.
                    Some(json) => buf.push(json),
                    None => write_json(&mut buf, tree),
                }
                buf.push(b"}");

                if !buf.full {
                    ok = code_web_render(buf.bytes.as_ptr(), buf.len, into.as_ptr(), into.len()) != 0;
                }
            }
            answer(out, c"RenderResult", ok);
            return;
        }

        // A class this module does not handle is null, not an error.
        let mut null_value = CodeValue {
            tag: NULL,
            heap: 0,
            number: 0.0,
            str_: core::ptr::null(),
            boolean: 0,
            items: core::ptr::null_mut(),
            keys: core::ptr::null(),
            len: 0,
        };
        core::ptr::copy_nonoverlapping(&mut null_value as *mut CodeValue, out, 1);
    }

    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo) -> ! {
        core::arch::wasm32::unreachable()
    }
}

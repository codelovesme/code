// The wasm half of a browser module, which is the same for all of them.
//
// **A module speaks particles, in both directions.** Toward the language
// that has always been true; toward the page it was not — a module used to
// reach its own world through pointers and lengths of its own devising, one
// shape per module, and had to take a particle apart to do it.
//
// So this half takes no particle apart at all. It writes the one it was
// given as JSON, hands it to the page under its module's name, and reads the
// particle that comes back. Every browser module's wasm half is that, which
// is why it is written once and included rather than typed five times:
//
// ```ignore
// include!("../../browser_half.rs");
// browser_half!("dom");
// ```
//
// Included rather than depended on: it is one file compiled into each module
// that wants it, not a crate between them, and a module that does not want
// it is unaffected by its existing.
//
// What it costs is that a browser module has no opinions of its own on this
// side. What it buys is that the interesting half — what a `Render` or a
// `Get` actually *means* — is written once, in the only language that can
// reach a page, instead of half in Rust and half in JavaScript.

use core::ffi::{c_char, c_void};

/// `CODE_VALUE_SLOT_SIZE`.
const NULL_TAG: i32 = 3;

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

extern "C" {
    /// The one door from any browser module to the page: a particle in as
    /// JSON, a particle out as JSON, under the module's own name. Answers how
    /// many bytes it wrote, or a negative number when it could not answer at
    /// all — which is the page saying "no such module here", never "the work
    /// failed". A failure that the module could describe comes back as an
    /// `Exception` particle like any other.
    fn code_web_ask(
        name: *const u8,
        name_len: i64,
        json: *const u8,
        json_len: i64,
        out: *mut u8,
        cap: i64,
    ) -> i64;
    /// The runtime's own, so a particle is spelled on the wire exactly as the
    /// language spells it — a number included.
    fn code_json_write(v: *const CodeValue, out: *mut u8, cap: i64) -> i64;
    fn code_json_read(text: *const u8, len: i64, out: *mut CodeValue) -> i32;
}

/// One particle in each direction. Fixed and static because `no_std` means
/// bringing an allocator of one's own, and a page is asked one thing at a
/// time — `code_web_ask` has returned before the next call can start.
const CAP: usize = 256 * 1024;
static mut ASKED: [u8; CAP] = [0; CAP];
static mut ANSWERED: [u8; CAP] = [0; CAP];

/// Writes null into `out` — what a module answers for a class it does not
/// handle, which is not an error: the particle may have been meant for
/// something else entirely.
///
/// # Safety
///
/// `out` must be a valid `CodeValue` slot.
pub unsafe fn null(out: *mut CodeValue) {
    let mut value = CodeValue {
        tag: NULL_TAG,
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

/// Hands `particle` to the page under `name` and writes back what it
/// answered.
///
/// Null on every path that is not an answer: a particle too big to write, a
/// page with no half for this module, an answer that is not a particle. Null
/// rather than an `Exception`, because none of those is this module failing
/// at its work — they are it not being asked, or not being there.
///
/// # Safety
///
/// Both pointers must be valid and laid out per `code_abi.h` — the host
/// guarantees this on every dispatch.
pub unsafe fn ask_the_page(name: &str, out: *mut CodeValue, particle: *const CodeValue) {
    let asked = &mut *core::ptr::addr_of_mut!(ASKED);
    let written = code_json_write(particle, asked.as_mut_ptr(), CAP as i64);
    if written <= 0 {
        null(out);
        return;
    }

    let answered = &mut *core::ptr::addr_of_mut!(ANSWERED);
    let read = code_web_ask(
        name.as_ptr(),
        name.len() as i64,
        asked.as_ptr(),
        written,
        answered.as_mut_ptr(),
        CAP as i64,
    );
    if read <= 0 {
        null(out);
        return;
    }

    null(out);
    if code_json_read(answered.as_ptr(), read, out) == 0 {
        null(out);
    }
}

/// Declares a browser module's two exports, under the prefix a `.a` is linked
/// by.
///
/// The prefix and the name the page knows it as are the same string, which is
/// the module's own name — one thing to get right rather than two.
#[macro_export]
macro_rules! browser_half {
    ($name:literal, $abi_version:ident, $dispatch:ident) => {
        #[no_mangle]
        pub extern "C" fn $abi_version() -> u32 {
            1
        }

        /// # Safety
        ///
        /// Both pointers must be valid for the duration of the call and laid
        /// out per `code_abi.h` — the host guarantees this on every dispatch.
        #[no_mangle]
        pub unsafe extern "C" fn $dispatch(out: *mut CodeValue, particle: *const CodeValue) {
            ask_the_page($name, out, particle)
        }
    };
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

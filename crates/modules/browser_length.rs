// Included only by arrays/strings. These pure handlers need no page service.
// Keep all other handlers on the ordinary browser_half JSON dispatch path.
use core::ffi::CStr;

#[repr(C)]
struct ListHead {
    len: i64,
    keys: *const *const core::ffi::c_char,
}

unsafe fn field(p: *const CodeValue, name: &str) -> Option<*const CodeValue> {
    if (*p).tag != 5 || (*p).payload == 0 {
        return None;
    }
    let head = (*p).payload as *const ListHead;
    for i in 0..(*head).len as usize {
        if CStr::from_ptr(*(*head).keys.add(i)).to_bytes() == name.as_bytes() {
            return Some((head.add(1) as *const u8).add(i * 16) as *const CodeValue);
        }
    }
    None
}

unsafe fn text<'a>(p: *const CodeValue) -> Option<&'a [u8]> {
    // Private helper: callers keep the borrow inside this dispatch.
    if (*p).tag != 1 {
        return None;
    }
    if (*p).payload == 0 {
        return Some(b"");
    }
    Some(CStr::from_ptr((*p).payload as *const core::ffi::c_char).to_bytes())
}

extern "C" {
    fn code_number(out: *mut CodeValue, value: f64);
    fn code_str(out: *mut CodeValue, text: *const u8);
    fn code_object(out: *mut CodeValue, keys: *const *const u8, values: *mut CodeValue, len: i64);
    fn code_release(value: *mut CodeValue);
}

unsafe fn try_length(
    name: &str,
    out: *mut CodeValue,
    p: *const CodeValue,
    tag: i32,
    error: &[u8],
) -> bool {
    if field(p, "_class").and_then(|v| text(v)) != Some(b"Length".as_slice()) {
        return false;
    }
    // Capacity -1 is an optional host preflight, never a particle. The grant
    // is larger than any wasm32 buffer length; it cannot be confused with an
    // old host's JSON length or any negative "no answer" return.
    if code_web_ask(
        name.as_ptr(),
        name.len() as i64,
        core::ptr::null(),
        0,
        core::ptr::null_mut(),
        -1,
    ) != 0x1_4c4f4341
    {
        return false;
    }
    let value = field(p, "value").filter(|v| (**v).tag == tag);
    let Some(value) = value else {
        null(out);
        code_json_read(error.as_ptr(), error.len() as i64, out);
        return true;
    };
    let count = if tag == 4 {
        if (*value).payload == 0 {
            0
        } else {
            (*((*value).payload as *const ListHead)).len as usize
        }
    } else {
        text(value)
            .unwrap()
            .iter()
            .filter(|b| (**b & 0xc0) != 0x80)
            .count()
    };
    let mut slots = [
        CodeValue {
            tag: 3,
            heap: 0,
            payload: 0,
        },
        CodeValue {
            tag: 3,
            heap: 0,
            payload: 0,
        },
    ];
    code_str(&mut slots[0], c"LengthResult".as_ptr().cast());
    code_number(&mut slots[1], count as f64);
    let keys = [
        c"_class".as_ptr().cast::<u8>(),
        c"value".as_ptr().cast::<u8>(),
    ];
    code_object(out, keys.as_ptr(), slots.as_mut_ptr(), 2);
    code_release(&mut slots[0]);
    code_release(&mut slots[1]);
    true
}

macro_rules! browser_length_half {
    ($name:literal, $tag:literal, $error:literal, $abi:ident, $dispatch:ident) => {
        #[no_mangle]
        pub extern "C" fn $abi() -> u32 {
            2
        }
        /// # Safety
        /// Pointers are initialized ABI 2 values valid during dispatch.
        #[no_mangle]
        pub unsafe extern "C" fn $dispatch(out: *mut CodeValue, particle: *const CodeValue) {
            if !try_length($name, out, particle, $tag, $error) {
                ask_the_page($name, out, particle);
            }
        }
    };
}

//! The fixture harness's `.a` static-module double.
//!
//! The other half of the ABI: a `.a` links straight into the host binary
//! rather than being `dlopen`'d, so there is no deep-copy boundary, no
//! per-module `code_release`, and exactly one runtime — the host's. What
//! there *is* instead is a symbol-prefixing requirement, since every `.a`
//! linked into one program shares a flat symbol table.
//!
//! The three entry points carry the `testmath_` prefix, chosen by this
//! module and unique among every `.a` a program might link alongside, so
//! `code build` can find them by running `nm` on the archive (see
//! `loader.rs`'s `static_module_symbols`). Nothing in the language names the
//! prefix — it just has to be unique.
//!
//! Rust since 2026-08-28, when `code-native` gained its `static-module`
//! feature; the C original is in this file's git history.

use std::sync::OnceLock;

use code_native::*;

#[no_mangle]
pub extern "C" fn testmath_code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// # Safety
///
/// Both pointers must be valid for the duration of the call and laid out per
/// `code_abi.h` — the host guarantees this on every dispatch.
#[no_mangle]
pub unsafe extern "C" fn testmath_code_module_dispatch(
    out: *mut CodeValue,
    particle: *const CodeValue,
) {
    let particle = &*particle;
    guarded(&mut *out, "test_math_static", |out| {
        if read_field_str(particle, "_class") != Some("Sum") {
            null(out);
            return;
        }
        let Some(items) = find_field(particle, "value") else {
            exception(out, "test_math_static", "Sum requires an array 'value'");
            return;
        };
        if items.tag != CodeTag::Array {
            exception(out, "test_math_static", "Sum requires an array 'value'");
            return;
        }
        let Some(numbers) = array_elems(items)
            .map(read_number)
            .collect::<Option<Vec<_>>>()
        else {
            exception(out, "test_math_static", "Sum requires an array of numbers");
            return;
        };
        let total: f64 = numbers.into_iter().sum();
        make_result(out, c"SumResult", |slot| number(slot, total));
    })
}

/// Exported variables (constants) — what `link "x.a" as m` exposes as
/// `m.<name>`. Owned for the program's whole lifetime: the host only ever
/// retains references into this (see `code_static_vars_object` in
/// `runtime.c`), never frees it.
static VARS: OnceLock<CodeVarList> = OnceLock::new();

#[no_mangle]
pub extern "C" fn testmath_code_module_vars() -> *const CodeVarList {
    VARS.get_or_init(|| {
        let mut buf = SlotBuffer::new(1);
        number(buf.slot_mut(0), 42.0);
        let values = buf.slot_mut(0) as *mut CodeValue;
        // Leaked deliberately, the same requirement a C module meets with
        // `static` storage.
        std::mem::forget(buf);
        let names: &'static [*const std::ffi::c_char] = Box::leak(Box::new([c"answer".as_ptr()]));
        CodeVarList {
            count: 1,
            names: names.as_ptr(),
            values,
        }
    })
}

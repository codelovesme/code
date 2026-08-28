//! A `.a` whose author picked two prefixes by mistake.
//!
//! Used only by `fail_native_link_static_ambiguous.code`, to exercise
//! `loader.rs`'s "a `.a` module's prefix must be unique" check
//! (`static_module_symbols`). Neither dispatch needs to do anything real —
//! `link` never gets far enough to call either.

use code_native::*;

#[no_mangle]
pub extern "C" fn foo_code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// # Safety
/// Never actually called; `link` fails before dispatch.
#[no_mangle]
pub unsafe extern "C" fn foo_code_module_dispatch(
    out: *mut CodeValue,
    _particle: *const CodeValue,
) {
    null(&mut *out)
}

#[no_mangle]
pub extern "C" fn bar_code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// # Safety
/// Never actually called; `link` fails before dispatch.
#[no_mangle]
pub unsafe extern "C" fn bar_code_module_dispatch(
    out: *mut CodeValue,
    _particle: *const CodeValue,
) {
    null(&mut *out)
}

use code_native::{borrowed::*, *};

#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// # Safety
/// The host borrows initialized ABI 2 values during dispatch.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, p: *const CodeValue) {
    match find_field(&*p, "value") {
        Some(v) => copy(&mut *out, v),
        None => null(&mut *out),
    }
}

/// # Safety
/// The view and callbacks are valid during this synchronous call.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch_borrowed_v1(
    out: *mut CodeValue,
    api: *const CodeBorrowedApi,
    p: *const core::ffi::c_void,
) -> core::ffi::c_int {
    let Some(p) = BorrowedValue::from_raw(api, p) else {
        return 0;
    };
    match p.field("_class").and_then(|v| v.text()) {
        Some("Inspect") => {
            let v = p.field("value").unwrap();
            let info = v.info();
            make_result(&mut *out, c"Inspected", |slot| {
                number(slot, info.length as f64)
            });
            1
        }
        // The engine must let a custom module give Length its own meaning.
        Some("Length") => {
            number(&mut *out, 777.0);
            1
        }
        _ => {
            // A declined result is released before the ordinary fallback.
            owned_str(&mut *out, "temporary decline result");
            0
        }
    }
}

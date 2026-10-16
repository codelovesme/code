//! Stateless clock queries, separate from timer's pending callbacks.
#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
    use code_native::*;

    #[no_mangle]
    pub extern "C" fn code_module_abi_version() -> u32 {
        CODE_ABI_VERSION
    }

    /// # Safety
    /// Both pointers must be valid ABI values for the duration of dispatch.
    #[no_mangle]
    pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
        let particle = &*particle;
        guarded(&mut *out, "time", |out| {
            match read_field_str(particle, "_class").unwrap_or("") {
                "Timestamp" => make_result(out, c"TimestampResult", |v| number(v, timestamp())),
                "TimezoneOffset" => {
                    make_result(out, c"TimezoneOffsetResult", |v| number(v, offset()))
                }
                _ => null(out),
            }
        });
    }

    fn timestamp() -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as f64
    }

    #[cfg(unix)]
    fn offset() -> f64 {
        let now = timestamp() as libc::time_t;
        // SAFETY: owned output storage and a live time_t match localtime_r's contract.
        let mut local: libc::tm = unsafe { std::mem::zeroed() };
        if unsafe { libc::localtime_r(&now, &mut local) }.is_null() {
            return 0.0;
        }
        local.tm_gmtoff as f64 / 60.0
    }
    #[cfg(not(unix))]
    fn offset() -> f64 {
        0.0
    }
}

#[cfg(target_arch = "wasm32")]
include!("../../browser_half.rs");
#[cfg(target_arch = "wasm32")]
browser_half!(
    "time",
    time_code_module_abi_version,
    time_code_module_dispatch
);

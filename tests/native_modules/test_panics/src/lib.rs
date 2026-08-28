//! A module that is wrong on purpose.
//!
//! Every handler here panics the way real Rust code panics — `unwrap` on
//! `None`, an index past the end, a division by zero — and none of them
//! takes the host down, because `code_module_dispatch` runs inside
//! [`guarded`].
//!
//! That guarantee cannot be provided by the host: a panic escaping an
//! `extern "C"` function aborts the process rather than unwinding, so the
//! host's own `catch_unwind` never runs. The catch has to be on this side of
//! the FFI boundary. This module exists to keep that true — delete the
//! `guarded` wrapper in `code-native` and `tests/panics_become_exceptions.code`
//! stops passing (by killing the test runner, loudly).

// Every lint below is firing on code that is wrong *on purpose* — this
// module's whole job is to be broken in the ways real modules are broken.
// Fixing them would delete the fixture.
#![allow(clippy::unnecessary_literal_unwrap, clippy::useless_vec)]

use code_native::*;

#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// # Safety
///
/// Both pointers must be valid for the duration of the call and laid out per
/// `code_abi.h` — the host guarantees this on every dispatch.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "test_panics", |out| {
        match read_field_str(particle, "_class").unwrap_or("") {
            "Unwrap" => {
                let nothing: Option<i32> = None;
                number(out, nothing.unwrap() as f64);
            }
            "Index" => {
                let three = vec![1, 2, 3];
                number(out, three[99] as f64);
            }
            // The divisor comes off the particle so this is a *runtime*
            // division by zero: a literal `100 / 0` does not compile, which
            // is itself the difference from the C side of this boundary,
            // where the same expression is a SIGFPE at runtime.
            "Divide" => {
                let by = read_field_number(particle, "by").unwrap_or(0.0) as i64;
                number(out, (100 / by) as f64);
            }
            // Not every handler is broken: the guard must not swallow the
            // ordinary path.
            "Fine" => make_result(out, c"FineResult", |slot| number(slot, 1.0)),
            _ => null(out),
        }
    })
}

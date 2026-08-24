//! The `math` native module — numeric operations for the Code programming
//! language, written in Rust on [`code-native`].
//!
//! Inherits the numeric half of `test_math` under the split proposal
//! (docs/todo/community-modules.md): `Double` and `Sum` stay here, while
//! `Shout`/`Echo` moved to `strings`. `test_math` itself stays a pure test
//! double with all four handlers, so the split costs it nothing.
//!
//! Handlers (each takes `{ "value": … }`, returns `<Name>Result`):
//!
//! - `Double` — `value * 2`, a Number back
//! - `Sum`    — the sum of an Array of Numbers; an empty array sums to 0
//!
//! Both operands are plain `f64`s end to end — the language's Number kind —
//! so there is no rounding, formatting, or overflow policy to decide here:
//! whatever IEEE 754 gives is what the caller gets. Non-finite results
//! cannot arise from source anyway (there are no `inf`/`nan` literals, and
//! division by zero is a runtime error before any emit happens).
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it (see that crate's docs for
//! why the rename-and-re-export dance exists at all).

use code_native::*;

/// The ABI version this module speaks. Must equal `CODE_ABI_VERSION` or the
/// host refuses to load us.
#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// The single dispatch point: read the particle's `_class`, route to the
/// matching handler. Unknown classes raise a fatal error naming the class —
/// the same wording shape `test_math` and `strings` use, so a mis-emitted
/// particle points at itself in both backends.
///
/// # Safety
///
/// Both pointers must be valid for reads/writes respectively for the
/// duration of the call, and refer to values laid out per `code_abi.h` —
/// the host guarantees this on every dispatch (see `native.rs`).
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    if particle.tag != CodeTag::Object {
        runtime_error("math: emit requires a particle");
    }
    let class = match read_field_str(particle, "_class") {
        Some(c) => c,
        None => runtime_error("math: emit requires a particle"),
    };
    let out = &mut *out;

    match class {
        "Double" => double(out, particle),
        "Sum" => sum(out, particle),
        other => runtime_error(&format!("math: unknown handler '{other}'")),
    }
}

// ---------------------------------------------------------------------------
// Shared operand extraction — every handler wants `value` off the particle,
// typed per-handler. Mirrors `strings`' per-field checks: a missing field
// and a wrong-typed field are different mistakes, and the errors say which.
// ---------------------------------------------------------------------------

fn require_value<'a>(particle: &'a CodeValue, class: &str) -> &'a CodeValue {
    match find_field(particle, "value") {
        Some(v) => v,
        None => runtime_error(&format!("{class} requires a 'value' field")),
    }
}

/// A Number operand — `Double`'s whole contract.
fn require_number(particle: &CodeValue, class: &str) -> f64 {
    let v = require_value(particle, class);
    match read_number(v) {
        Some(n) => n,
        None => runtime_error(&format!("{class} requires a numeric 'value'")),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Multiply by two. Byte-for-byte parity with `test_math`'s `Double`
/// (which this module inherits in the split proposal): one multiply, one
/// Number back, no surprises.
fn double(out: &mut CodeValue, particle: &CodeValue) {
    let n = require_number(particle, "Double");
    make_result(out, c"DoubleResult", |slot| number(slot, n * 2.0));
}

/// Sum an array of Numbers. An empty array sums to 0 — the identity of the
/// operation, and exactly what `test_math`'s accumulator does. Non-number
/// elements are refused rather than coerced: inventing a string-to-number
/// parsing policy here would be worse than saying the input was wrong.
fn sum(out: &mut CodeValue, particle: &CodeValue) {
    let items = require_value(particle, "Sum");
    if items.tag != CodeTag::Array {
        runtime_error("Sum requires an array 'value'");
    }
    let total: f64 = array_elems(items)
        .map(read_number)
        .collect::<Option<Vec<_>>>()
        .unwrap_or_else(|| runtime_error("Sum requires an array of numbers"))
        .into_iter()
        .sum();
    make_result(out, c"SumResult", |slot| number(slot, total));
}

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
    // `guarded` so a panic anywhere below becomes an `Exception` rather than
    // taking the host down; the `Err` arm turns a handler's own refusal into
    // the same shape, so both kinds of failure reach the caller as a value.
    guarded(&mut *out, "math", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Double" => double(out, particle),
            "Sum" => sum(out, particle),
            // A class this module does not handle answers null — whether to
            // act on a particle is the recipient's business.
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "math", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Shared operand extraction — every handler wants `value` off the particle,
// typed per-handler. Mirrors `strings`' per-field checks: a missing field
// and a wrong-typed field are different mistakes, and the messages say
// which. Unlike `net`, there is nothing here to *attempt* and let fail —
// arithmetic on a non-number has no operation to reach — so these stay
// explicit, and become the `Exception`'s message.
// ---------------------------------------------------------------------------

/// A Number operand — `Double`'s whole contract.
///
/// A field the particle does not carry is null — the same answer `.field`
/// gives — so there is no separate "you didn't supply it" case. Emitting a
/// particle is not a form to be validated before the handler may run
/// (owner's rule, 2026-08-28); `net` was rewritten around this in phase 2 and
/// this is the rest of the modules catching up.
fn require_number(particle: &CodeValue, class: &str) -> Result<f64, String> {
    find_field(particle, "value")
        .and_then(read_number)
        .ok_or_else(|| format!("{class} requires a numeric 'value'"))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Multiply by two. Byte-for-byte parity with `test_math`'s `Double`
/// (which this module inherits in the split proposal): one multiply, one
/// Number back, no surprises.
fn double(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let n = require_number(particle, "Double")?;
    make_result(out, c"DoubleResult", |slot| number(slot, n * 2.0));
    Ok(())
}

/// Sum an array of Numbers. An empty array sums to 0 — the identity of the
/// operation, and exactly what `test_math`'s accumulator does. Non-number
/// elements are refused rather than coerced: inventing a string-to-number
/// parsing policy here would be worse than saying the input was wrong.
fn sum(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let items = find_field(particle, "value")
        .filter(|v| v.tag == CodeTag::Array)
        .ok_or("Sum requires an array 'value'")?;
    let total: f64 = array_elems(items)
        .map(read_number)
        .collect::<Option<Vec<_>>>()
        .ok_or("Sum requires an array of numbers")?
        .into_iter()
        .sum();
    make_result(out, c"SumResult", |slot| number(slot, total));
    Ok(())
}

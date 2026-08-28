//! The fixture harness's general-purpose test double.
//!
//! Covers the shapes a real module needs: a Number result (`Double`), a
//! fresh heap-owned Str result (`Shout`), a reduction over an Array
//! (`Sum`), and unchanged passthrough of whatever `value` was handed in,
//! including nested Array/Object (`Echo`). Plus exported variables covering
//! **all six value kinds**, so the deep-copy-at-the-boundary path
//! (`code_native_copy_in` / `ffi_to_value`) is exercised for each.
//!
//! Deliberately keeps all four handlers even though `Shout`/`Echo` moved to
//! the real `strings` module and `Double`/`Sum` to `math`: this is a pure
//! test double, so the split cost it nothing.

use std::sync::OnceLock;

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
    guarded(&mut *out, "test_math", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Double" => double(out, particle),
            "Shout" => shout(out, particle),
            "Sum" => sum(out, particle),
            "Echo" => echo(out, particle),
            // A class this module does not handle answers null — whether to
            // act on a particle is the recipient's business.
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "test_math", &message);
        }
    })
}

fn double(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let n = read_field_number(particle, "value").ok_or("Double requires a numeric 'value'")?;
    make_result(out, c"DoubleResult", |slot| number(slot, n * 2.0));
    Ok(())
}

/// Uppercase ASCII letters and append `!` — a result string built fresh
/// rather than borrowed, which is the point of having it here.
fn shout(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let s = read_field_str(particle, "value").ok_or("Shout requires a string 'value'")?;
    let shouted: String = s
        .chars()
        .map(|c| c.to_ascii_uppercase())
        .collect::<String>()
        + "!";
    make_result(out, c"ShoutResult", |slot| owned_str(slot, &shouted));
    Ok(())
}

fn sum(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let items = find_field(particle, "value").ok_or("Sum requires a 'value' field")?;
    if items.tag != CodeTag::Array {
        return Err("Sum requires an array 'value'".to_string());
    }
    let total: f64 = array_elems(items)
        .map(read_number)
        .collect::<Option<Vec<_>>>()
        .ok_or("Sum requires an array of numbers")?
        .into_iter()
        .sum();
    make_result(out, c"SumResult", |slot| number(slot, total));
    Ok(())
}

/// Unchanged passthrough, including nested Array/Object — a handler that
/// shares structure with its input rather than building something fresh,
/// which every other handler here does.
fn echo(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let value = find_field(particle, "value").ok_or("Echo requires a 'value' field")?;
    make_result(out, c"EchoResult", |slot| copy(slot, value));
    Ok(())
}

// ---------------------------------------------------------------------------
// Exported variables — what `link "x.so" as m` exposes as `m.<name>`.
// ---------------------------------------------------------------------------

/// Built once, on first ask, and never freed: these are the module's own
/// permanent constants, owned for its whole lifetime. The host deep-copies
/// each value out at `link` time, and `code_native_close` never `dlclose`s,
/// so they outlive the object borrowing their key strings.
///
/// Leaked deliberately, the same requirement a C module meets with `static`
/// storage — see `code-native`'s README.
static VARS: OnceLock<CodeVarList> = OnceLock::new();

#[no_mangle]
pub extern "C" fn code_module_vars() -> *const CodeVarList {
    VARS.get_or_init(|| {
        let mut buf = SlotBuffer::new(6);
        number(buf.slot_mut(0), 42.0); // Number
        borrowed_str(buf.slot_mut(1), c"test_math"); // Str
        boolean(buf.slot_mut(2), true); // Bool
        null(buf.slot_mut(3)); // Null

        // factors = [2, 3, 5]
        let mut elems = SlotBuffer::new(3);
        number(elems.slot_mut(0), 2.0);
        number(elems.slot_mut(1), 3.0);
        number(elems.slot_mut(2), 5.0);
        array(buf.slot_mut(4), &mut elems);
        elems.release_all();

        // meta = { "version": 1, "owner": "test" }
        let mut fields = SlotBuffer::new(2);
        number(fields.slot_mut(0), 1.0);
        borrowed_str(fields.slot_mut(1), c"test");
        object(buf.slot_mut(5), &[c"version", c"owner"], &mut fields);
        fields.release_all();

        let values = buf.slot_mut(0) as *mut CodeValue;
        std::mem::forget(buf);
        let names: &'static [*const std::ffi::c_char] = Box::leak(Box::new([
            c"answer".as_ptr(),
            c"name".as_ptr(),
            c"enabled".as_ptr(),
            c"nothing".as_ptr(),
            c"factors".as_ptr(),
            c"meta".as_ptr(),
        ]));
        CodeVarList {
            count: 6,
            names: names.as_ptr(),
            values,
        }
    })
}

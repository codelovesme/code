//! A `.a` static module that pushes particles into the program.
//!
//! `test_events` proves the inbound half of the boundary for a `.so`. This
//! proves it for the other half, which until 2026-08-28 had no queue at all
//! — not by decision, but because a `.a` is linked straight into the host and
//! so has no `dlopen` handle, and the handle was where the queue lived.
//! `code_static_open` now allocates one for exactly this.
//!
//! Everything else about the static ABI applies unchanged: one runtime (the
//! host's), no deep-copy boundary, and the symbol-prefix requirement, since
//! every `.a` in one program shares a flat symbol table. The prefix here is
//! `testevents_`, and `code build` finds these by running `nm` on the archive
//! (`loader.rs`'s `static_module_symbols`) — including
//! `testevents_code_module_set_inbound`, whose presence is the whole signal
//! that this module intends to speak.

use code_native::*;

#[no_mangle]
pub extern "C" fn testevents_code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

// Spelled out rather than assembled from a prefix: `macro_rules!` cannot
// concatenate identifiers, and a static module writes its other exports the
// same way.
declare_inbound!(testevents_code_module_set_inbound);

/// # Safety
///
/// Both pointers must be valid for the duration of the call and laid out per
/// `code_abi.h` — the host guarantees this on every dispatch.
#[no_mangle]
pub unsafe extern "C" fn testevents_code_module_dispatch(
    out: *mut CodeValue,
    particle: *const CodeValue,
) {
    let particle = &*particle;
    guarded(&mut *out, "test_events_static", |out| {
        if read_field_str(particle, "_class") != Some("Start") {
            // Including `Tick`, which this module pushes but never answers.
            null(out);
            return;
        }
        let Some(count) = read_field_number(particle, "value") else {
            exception(out, "test_events_static", "Start requires a numeric 'value'");
            return;
        };
        let count = count as i64;
        for i in 0..count {
            let mut tick = CodeValue::zeroed();
            make_result(&mut tick, c"Tick", |slot| number(slot, i as f64));
            // Best effort, exactly as for a `.so`: a host that took no
            // inbound channel simply never hears these.
            emit_inbound(&tick);
            release(&mut tick);
        }
        make_result(out, c"StartedResult", |slot| number(slot, count as f64));
    })
}

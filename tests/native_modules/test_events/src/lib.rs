//! A module that pushes particles *into* the program rather than only
//! answering it — the inbound half of the boundary (see
//! `docs/todo/inbound-emissions-from-native-modules.md`).
//!
//! `Start { value: n }` queues n `Tick` particles and answers
//! `StartedResult`. The queue and the pusher are handed over by the host at
//! link time through the `code_module_set_inbound` export that
//! [`declare_inbound!`] generates; a module that never speaks first simply
//! does not invoke that macro.

use code_native::*;

#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

declare_inbound!();

/// # Safety
///
/// Both pointers must be valid for the duration of the call and laid out per
/// `code_abi.h` — the host guarantees this on every dispatch.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "test_events", |out| {
        if read_field_str(particle, "_class") != Some("Start") {
            // Including `Tick`, which this module pushes but never answers.
            null(out);
            return;
        }
        let Some(count) = read_field_number(particle, "value") else {
            exception(out, "test_events", "Start requires a numeric 'value'");
            return;
        };
        let count = count as i64;
        for i in 0..count {
            let mut tick = CodeValue::zeroed();
            make_result(&mut tick, c"Tick", |slot| number(slot, i as f64));
            // Best effort: a host that took no inbound channel simply never
            // hears these, and this module stays correct either way.
            emit_inbound(&tick);
            release(&mut tick);
        }
        make_result(out, c"StartedResult", |slot| number(slot, count as f64));
    })
}

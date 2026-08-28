//! A module with a thread of its own — the half of inbound emissions that
//! `test_events` cannot reach (see
//! `docs/todo/inbound-emissions-from-native-modules.md`).
//!
//! `test_events` pushes from inside the `code_module_dispatch` call it was
//! asked on, so it is still on the program's thread and the program is still
//! inside an `emit`. This one answers *first* and pushes afterwards, from a
//! thread the program knows nothing about: the ticks arrive while the program
//! is somewhere else entirely, which is what an interactive module (a
//! terminal reading keys, a socket accepting connections) actually does.
//!
//! `Start { value: n }` answers `StartedResult` immediately and spawns a
//! thread that pushes n `Tick` particles, one every millisecond.

use code_native::*;
use std::thread;
use std::time::Duration;

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
    guarded(&mut *out, "test_timer", |out| {
        if read_field_str(particle, "_class") != Some("Start") {
            // Including `Tick`, which this module pushes but never answers.
            null(out);
            return;
        }
        let Some(count) = read_field_number(particle, "value") else {
            exception(out, "test_timer", "Start requires a numeric 'value'");
            return;
        };
        let count = count as i64;
        // Detached on purpose: nothing joins it, and the program may well
        // finish while it is still going. That is the case the host side has
        // to survive — see `code_native_close` in `runtime.c` and
        // `Drop for NativeModule` in `native.rs`, which both leave a module
        // that can speak first mapped rather than pulling it out from under
        // this thread.
        thread::spawn(move || {
            for i in 0..count {
                // Long enough that the program is provably somewhere else by
                // the time this lands — a push from inside `dispatch` would
                // prove nothing.
                thread::sleep(Duration::from_millis(1));
                let mut tick = CodeValue::zeroed();
                make_result(&mut tick, c"Tick", |slot| number(slot, i as f64));
                // Best effort, as always: a host that took no inbound channel
                // simply never hears these.
                emit_inbound(&tick);
                release(&mut tick);
            }
        });
        make_result(out, c"StartedResult", |slot| number(slot, count as f64));
    })
}

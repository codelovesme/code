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
use std::sync::Mutex;

#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

declare_inbound!();
declare_inbound_reply!(answered);

/// What the program answered to the last `Tick` this module pushed, and how
/// many answers have arrived. A pushed particle's answer is the handler's
/// return value, handed back through `code_module_inbound_reply` — see
/// `code_abi.h`. Kept here so a fixture can ask for it and prove the answer
/// crossed back.
static ANSWERS: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));

/// The host, telling this module what the program returned. `result` is a
/// null value when nothing handled the push.
fn answered(_particle: &CodeValue, result: &CodeValue) {
    let value = find_field(result, "value").and_then(read_number).unwrap_or(-1.0);
    let mut answers = ANSWERS.lock().unwrap_or_else(|e| e.into_inner());
    answers.0 += 1.0;
    answers.1 = value;
}


/// # Safety
///
/// Both pointers must be valid for the duration of the call and laid out per
/// `code_abi.h` — the host guarantees this on every dispatch.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "test_events", |out| {
        // What the program answered to the pushes so far: `{ count, last }`.
        if read_field_str(particle, "_class") == Some("Answers") {
            let (count, last) = *ANSWERS.lock().unwrap_or_else(|e| e.into_inner());
            let mut buf = SlotBuffer::new(3);
            borrowed_str(buf.slot_mut(0), c"AnswersResult");
            number(buf.slot_mut(1), count);
            number(buf.slot_mut(2), last);
            object(out, &[c"_class", c"count", c"last"], &mut buf);
            buf.release_all();
            return;
        }
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

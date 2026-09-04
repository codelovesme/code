//! The `membrane` native module — an application's boundary when a host is
//! holding it.
//!
//! A cell's membrane is where it touches whatever is outside. That is all
//! this is: the place a held application's traffic arrives, standing where
//! its own door would be.
//!
//! **The vocabulary is `net_server`'s, word for word**, and that is the whole
//! design:
//!
//! - `Config { … }` → `ConfigResult { ok }`
//! - `Listen {}`    → `ListenResult { ok, port, message }`
//! - `Stop {}`      → `StopResult { ok }`
//!
//! So moving an application from running on its own to being held is one
//! word in its manifest — `net_server` becomes `membrane` — and not a line
//! of its genes. It still says "start listening"; what changes is what that
//! means.
//!
//! **It opens no socket, holds no thread, and pushes nothing.** That is not
//! a limitation, it is the point: those three are exactly what stops an
//! application from being held. A door of its own has a thread that outlives
//! the application, and a thread that outlives it cannot be unloaded, so its
//! memory never comes back. A membrane has none, so an application wearing
//! one can be started and stopped cleanly.
//!
//! **Held, this code does not run at all.** A host answers `membrane` itself
//! and hands the application a stand-in with these same three handlers — see
//! `code_abi.h` item 10. Its `Listen` registers the application for traffic
//! rather than binding anything, and the host's own door routes to it by
//! name.
//!
//! So what is left here is the standalone case, and there the honest answer
//! is that there is nobody to be held by. `Listen` says so rather than
//! pretending: an application built to be held, run on its own, finds out at
//! the line where it asks to start.

use code_native::*;

/// The ABI version this module speaks. Must equal `CODE_ABI_VERSION` or the
/// host refuses to load us.
#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// The single dispatch point: read the particle's `_class`, route to the
/// matching handler.
///
/// # Safety
///
/// Both pointers must be valid for reads/writes respectively for the
/// duration of the call, and refer to values laid out per `code_abi.h` —
/// the host guarantees this on every dispatch.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "membrane", |out| {
        match read_field_str(particle, "_class").unwrap_or("") {
            // Accepted and ignored, exactly as a host accepts and ignores it:
            // what a held application is configured with is the host's to
            // decide, and on its own there is nothing here to configure.
            "Config" => config_result(out, true),
            "Listen" => listen_result(out),
            // Nothing was started, so stopping is nothing to do — and
            // answering `ok` keeps a shutdown path that works either way.
            "Stop" => stop_result(out, true),
            // A class this module does not handle answers null — whether to
            // act on a particle is the recipient's business.
            _ => null(out),
        }
    })
}

fn config_result(out: &mut CodeValue, ok: bool) {
    let mut buf = SlotBuffer::new(2);
    borrowed_str(buf.slot_mut(0), c"ConfigResult");
    boolean(buf.slot_mut(1), ok);
    object(out, &[c"_class", c"ok"], &mut buf);
    buf.release_all();
}

/// `Listen {}` → `ListenResult { ok, port, message }`.
///
/// Reached only when nobody is holding this application: a host answers
/// `membrane` before this file is ever opened. So the answer is no, with the
/// reason — the alternative is to say yes and then never deliver anything,
/// which is the same failure with the evidence removed.
fn listen_result(out: &mut CodeValue) {
    let mut buf = SlotBuffer::new(4);
    borrowed_str(buf.slot_mut(0), c"ListenResult");
    boolean(buf.slot_mut(1), false);
    number(buf.slot_mut(2), 0.0);
    borrowed_str(
        buf.slot_mut(3),
        c"no host: this application's door is a membrane, which only means \
          something inside a host. Running it on its own wants net_server.",
    );
    object(out, &[c"_class", c"ok", c"port", c"message"], &mut buf);
    buf.release_all();
}

fn stop_result(out: &mut CodeValue, ok: bool) {
    let mut buf = SlotBuffer::new(2);
    borrowed_str(buf.slot_mut(0), c"StopResult");
    boolean(buf.slot_mut(1), ok);
    object(out, &[c"_class", c"ok"], &mut buf);
    buf.release_all();
}

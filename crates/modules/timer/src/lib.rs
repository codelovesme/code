//! The `timer` native module — a particle, later. In a browser and on a machine.
//!
//! Handlers:
//!
//! - `Delay { ms, then }` — answers `DelayResult { value }`, the number this
//!   delay is known by. After `ms` milliseconds, `then` arrives: a whole
//!   particle, written where the delay is asked for, or just a class name
//!   when there is nothing else to say.
//! - `Cancel { id }` — answers `CancelResult { ok }`. Cancelling one that
//!   has already fired, or was never started, is `ok = false` rather than a
//!   failure: it means the same thing either way.
//!
//! ```text
//! emit Delay { ms = 5000, then = { _class = "Refresh", what = "prices" } } to timer get d
//!
//! Refresh { what } => {
//!     ...and re-arm, if it should keep going
//! }
//! ```
//!
//! The application names the particle it wants back, in advance — the same
//! rule `dom` follows for a click and `router` for a path. It is not a
//! boundary; see `router`'s file for why there cannot be one between a page
//! and the module it loaded.
//!
//! **Nothing repeats on its own.** A delay fires once; a handler that wants
//! a heartbeat asks for the next one itself. Repeating would mean a timer
//! outliving the reason it was started, which is how a program ends up doing
//! work nobody asked for — and re-arming is one line at the end of the
//! handler that already ran.
//!
//! # It does not hold the program open
//!
//! On a machine a program ends at its last statement unless a module says it
//! is still serving (`code_abi.h` item 8). A pending delay does not say
//! that: an application that wants to stay up is serving something — a
//! socket, a queue — and a timer is not that thing. So a program whose only
//! module is this one ends, with its delay unfired, which is what it asked
//! for by having nothing else to do.
//!
//! # Where it works
//!
//! **A browser today.** On a machine every handler answers an `Exception`
//! saying so; the thread-and-queue half is written down as the next step and
//! not yet built, since what needed a timer first was a page.
//!
//! For wasm it is built as an archive linked into the program:
//!
//! ```bash
//! cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
//! ```
//!
//! # What the page has to supply
//!
//! A half that answers this module's particles, in
//! [`web/host.mjs`](../../../web/README.md) — which `code build --target
//! wasm` writes beside the module it built, holding the halves of exactly
//! the modules the program linked.
//!
//! Nothing crosses as a pointer or a length: the particle goes over as JSON
//! and the answer comes back the same way, through one door shared by every
//! browser module. So this module's wasm half has nothing to do but hand the
//! particle over — see `crates/modules/browser_half.rs`, which is that half,
//! written once.
//!
//! Its wasm half is `no_std` and hand-written against `code_abi.h` — see
//! `dom`'s file for why a module meant for the browser brings no standard
//! library.

#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
    //! The machine half: a thread that sleeps, then pushes `then` onto the
    //! program's inbound ring. `Delay` answers at once with the number the
    //! delay is known by; the handler for `then` runs on the program's own
    //! thread when its loop next drains — never on this thread, never inside
    //! another handler. `Cancel` marks the number so the push is skipped.
    //!
    //! A pending delay holds the program open (`code_module_serving`),
    //! since a thread of this module is alive, and a host must never unmap
    //! one; `Cancel` wakes that thread at once so a stopping application is
    //! free to unload the moment it has cancelled what it armed.
    use code_native::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    /// A pending delay: the flag its thread waits on. `Cancel` sets it and
    /// wakes the thread, which then leaves without pushing — at once, not
    /// when the sleep would have ended. That is what lets a host unload an
    /// application that cancelled its delays: no thread of this module is
    /// still asleep inside code about to be unmapped.
    type Flag = Arc<(Mutex<bool>, Condvar)>;
    struct Pending {
        flag: Flag,
        /// Taken by `Cancel`, which joins it: when `Cancel` answers, the
        /// thread is gone, not merely told to go — so a host that unloads
        /// right after finds nothing of this module still running.
        thread: Option<thread::JoinHandle<()>>,
    }
    static PENDING: Mutex<Option<HashMap<u64, Pending>>> = Mutex::new(None);

    fn with_pending<R>(f: impl FnOnce(&mut HashMap<u64, Pending>) -> R) -> R {
        let mut guard = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        f(guard.get_or_insert_with(HashMap::new))
    }

    /// Non-zero while a delay is pending — a thread of this module is alive
    /// — so a host never unmaps a sleeping thread. It also keeps a standalone
    /// program up until its last delay fires or is cancelled: a delay is
    /// something the program asked for, and ending before it lands would be
    /// dropping it.
    #[no_mangle]
    pub extern "C" fn code_module_serving() -> std::ffi::c_int {
        i32::from(with_pending(|p| !p.is_empty()))
    }

    const DELAY_ID_FIELD: &str = "_delay_id";

    declare_inbound!();
    declare_inbound_reply!(answered);

    /// The program answered the `then` it was handed. Nothing waits on it.
    fn answered(_particle: &CodeValue, _result: &CodeValue) {}

    /// A value of the program's, made this module's own. `copy` shares the
    /// program's storage by reference, and a particle literal's storage does
    /// not outlive the dispatch that carried it — a thread that pushes it
    /// later would push freed memory. So every string, number, list and
    /// object is rebuilt here, and the copy is what the thread holds.
    fn own(v: &CodeValue) -> CodeValue {
        let mut out = CodeValue::zeroed();
        match v.tag {
            CodeTag::Str => owned_str(&mut out, read_str(v).unwrap_or("")),
            CodeTag::Number => number(&mut out, read_number(v).unwrap_or(0.0)),
            CodeTag::Bool => boolean(&mut out, read_bool(v).unwrap_or(false)),
            CodeTag::Array => {
                let mut buf = SlotBuffer::new(v.len as usize);
                for i in 0..v.len {
                    let item = unsafe { &*slot_at(v.items, i) };
                    let mut owned = own(item);
                    copy(buf.slot_mut(i), &owned);
                    release(&mut owned);
                }
                array(&mut out, &mut buf);
                buf.release_all();
            }
            CodeTag::Object => {
                let entries: Vec<(String, CodeValue)> = object_entries(v).map(|(k, x)| (k.to_string(), own(x))).collect();
                let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
                let mut buf = SlotBuffer::new(keys.len());
                for (i, (_, x)) in entries.iter().enumerate() {
                    copy(buf.slot_mut(i as i64), x);
                }
                object_dyn(&mut out, &keys, &mut buf);
                buf.release_all();
                for (_, mut x) in entries {
                    release(&mut x);
                }
            }
            CodeTag::Null => null(&mut out),
        }
        out
    }

    /// `then` as the particle to push: a class name becomes `{ _class }`, an
    /// object is taken whole; either way `_delay_id` is added so a handler
    /// can tell which delay it is answering.
    fn particle_from_then(then: &CodeValue, id: u64) -> Option<CodeValue> {
        let mut entries: Vec<(String, CodeValue)> = Vec::new();
        if let Some(class) = read_str(then) {
            if class.is_empty() {
                return None;
            }
            let mut c = CodeValue::zeroed();
            owned_str(&mut c, class);
            entries.push(("_class".to_string(), c));
        } else if then.tag == CodeTag::Object {
            let mut has_class = false;
            for (k, v) in object_entries(then) {
                if k == DELAY_ID_FIELD {
                    continue;
                }
                if k == "_class" {
                    has_class = true;
                }
                entries.push((k.to_string(), own(v)));
            }
            if !has_class {
                for (_, mut v) in entries {
                    release(&mut v);
                }
                return None;
            }
        } else {
            return None;
        }
        let mut keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
        keys.push(DELAY_ID_FIELD);
        let mut buf = SlotBuffer::new(keys.len());
        for (i, (_, v)) in entries.iter().enumerate() {
            copy(buf.slot_mut(i as i64), v);
        }
        number(buf.slot_mut(entries.len() as i64), id as f64);
        let mut particle = CodeValue::zeroed();
        object_dyn(&mut particle, &keys, &mut buf);
        buf.release_all();
        for (_, mut v) in entries {
            release(&mut v);
        }
        Some(particle)
    }

    fn handle_delay(out: &mut CodeValue, particle: &CodeValue) {
        let ms = match read_field_number(particle, "ms") {
            Some(ms) if ms >= 0.0 => ms,
            _ => {
                exception(out, "timer", "Delay needs `ms`, a number of milliseconds, zero or more");
                return;
            }
        };
        let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
        let then = match find_field(particle, "then").and_then(|t| particle_from_then(t, id)) {
            Some(p) => p,
            None => {
                exception(out, "timer", "Delay needs `then`: a class name, or a particle with `_class`");
                return;
            }
        };
        let pending: Flag = Arc::new((Mutex::new(false), Condvar::new()));
        // Filed before the thread starts, so a `Cancel` that lands between the
        // two finds it; the thread's own removal at the end is what frees a
        // fired delay, and `Cancel` takes the entry for a cancelled one.
        with_pending(|p| {
            p.insert(id, Pending { flag: Arc::clone(&pending), thread: None });
        });
        // The particle crosses to the thread whole and is released there
        // after the push; `CodeValue` is `Send` for exactly this.
        let carried = then;
        let handle = thread::spawn(move || {
            let mut carried = carried;
            let deadline = Instant::now() + Duration::from_millis(ms as u64);
            let (flag, wake) = &*pending;
            let mut cancelled = flag.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if *cancelled {
                    break;
                }
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                cancelled = wake
                    .wait_timeout(cancelled, deadline - now)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
            let was_cancelled = *cancelled;
            drop(cancelled);
            if !was_cancelled {
                emit_inbound(&carried);
            }
            release(&mut carried);
            with_pending(|p| {
                p.remove(&id);
            });
        });
        // The thread may have fired and removed itself already; then there
        // is nothing to file the handle under, and nothing to join later.
        with_pending(|p| {
            if let Some(entry) = p.get_mut(&id) {
                entry.thread = Some(handle);
            }
        });
        make_result(out, c"DelayResult", |slot| number(slot, id as f64));
    }

    fn handle_cancel(out: &mut CodeValue, particle: &CodeValue) {
        let id = read_field_number(particle, "id").map(|n| n as u64);
        let ok = match id {
            Some(id) => {
                // Taken out of the table before the wake, and joined without
                // the table's lock held — the thread's last act takes that
                // lock itself. When this answers, no thread of the delay is
                // left, which is what `code_module_serving` then reports.
                let taken = with_pending(|p| p.remove(&id));
                match taken {
                    Some(Pending { flag, thread: handle }) => {
                        let (cancelled, wake) = &*flag;
                        *cancelled.lock().unwrap_or_else(|e| e.into_inner()) = true;
                        wake.notify_all();
                        if let Some(handle) = handle {
                            let _ = handle.join();
                        }
                        true
                    }
                    None => false,
                }
            }
            None => false,
        };
        // `CancelResult { ok }`, the browser half's shape — not `value`.
        let mut buf = SlotBuffer::new(2);
        borrowed_str(buf.slot_mut(0), c"CancelResult");
        boolean(buf.slot_mut(1), ok);
        object(out, &[c"_class", c"ok"], &mut buf);
        buf.release_all();
    }

    #[no_mangle]
    pub extern "C" fn code_module_abi_version() -> u32 {
        CODE_ABI_VERSION
    }

    /// # Safety
    ///
    /// Both pointers must be valid for the duration of the call and laid out
    /// per `code_abi.h` — the host guarantees this on every dispatch.
    #[no_mangle]
    pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
        let particle = &*particle;
        guarded(&mut *out, "timer", |out| {
            match read_field_str(particle, "_class") {
                Some("Delay") => handle_delay(out, particle),
                Some("Cancel") => handle_cancel(out, particle),
                // A class this module does not handle answers null and does
                // not end the program: it may have been meant for something
                // else entirely.
                _ => null(out),
            }
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod page {
    include!("../../browser_half.rs");

    browser_half!(
        "timer",
        timer_code_module_abi_version,
        timer_code_module_dispatch
    );
}

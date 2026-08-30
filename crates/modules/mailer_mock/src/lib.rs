//! The `mailer_mock` native module — a drop-in for `mailer` that never
//! touches an SMTP server, for the Code programming language, written in
//! Rust on [`code-native`].
//!
//! Same particles as `mailer` — `Config { host, from, … }` → `ConfigResult
//! { ok }`, `Send { recipient, subject?, text?, html?, cc?, bcc? }` →
//! `SendResult { ok }` — but `Send` files the message into an in-memory
//! outbox rather than delivering it. Every SMTP field on `Config` is
//! accepted and ignored.
//!
//! One handler beyond `mailer`'s surface, for tests:
//!
//! - `Outbox {}` → `Outbox { messages, count }` — everything `Send` has
//!   captured since load, newest last. `Outbox { clear = true }` empties it
//!   after reading.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use code_native::*;
use std::sync::Mutex;

#[derive(Clone, Default)]
struct Sent {
    from: String,
    recipient: String,
    cc: String,
    bcc: String,
    subject: String,
    body: String,
    html: bool,
}

static CONFIGURED: Mutex<Option<String>> = Mutex::new(None); // the `from` address
static OUTBOX: Mutex<Vec<Sent>> = Mutex::new(Vec::new());

/// The ABI version this module speaks. Must equal `CODE_ABI_VERSION` or the
/// host refuses to load us.
#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// The single dispatch point: read `_class`, route to a handler. An
/// unhandled class is null; a handler that cannot do the work returns an
/// `Exception`. Neither ends the program.
///
/// # Safety
///
/// Both pointers must be valid for reads/writes for the duration of the
/// call and laid out per `code_abi.h` — the host guarantees this.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "mailer_mock", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Config" => config(out, particle),
            "Send" => send(out, particle),
            "Outbox" => outbox(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "mailer_mock", &message);
        }
    })
}

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    // `mailer` requires both; a mock that accepted less would let a broken
    // manifest pass in dev and fail in prod.
    require_str(particle, "host", "Config")?;
    let from = require_str(particle, "from", "Config")?.to_string();
    *CONFIGURED.lock().unwrap_or_else(|e| e.into_inner()) = Some(from);

    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"ConfigResult");
    boolean(b.slot_mut(1), true);
    object(out, &[c"_class", c"ok"], &mut b);
    b.release_all();
    Ok(())
}

fn send(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let default_from = CONFIGURED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or("mailer_mock has no transport — send Config { host, from } first")?;

    let recipient = joined(particle, "recipient");
    if recipient.is_empty() {
        return Err("Send needs a 'recipient'".to_string());
    }
    let html = find_field(particle, "html").and_then(read_str);
    let text = find_field(particle, "text").and_then(read_str);

    OUTBOX.lock().unwrap_or_else(|e| e.into_inner()).push(Sent {
        from: opt(particle, "from").unwrap_or(default_from),
        recipient,
        cc: joined(particle, "cc"),
        bcc: joined(particle, "bcc"),
        subject: opt(particle, "subject").unwrap_or_default(),
        body: html.or(text).unwrap_or("").to_string(),
        html: html.is_some(),
    });

    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"SendResult");
    boolean(b.slot_mut(1), true);
    object(out, &[c"_class", c"ok"], &mut b);
    b.release_all();
    Ok(())
}

fn outbox(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let mut guard = OUTBOX.lock().unwrap_or_else(|e| e.into_inner());

    let mut arr = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(guard.len());
    for (i, m) in guard.iter().enumerate() {
        message_value(buf.slot_mut(i as i64), m);
    }
    array(&mut arr, &mut buf);
    buf.release_all();

    let count = guard.len() as f64;
    if read_field_bool(particle, "clear").unwrap_or(false) {
        guard.clear();
    }
    drop(guard);

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"Outbox");
    copy(b.slot_mut(1), &arr);
    number(b.slot_mut(2), count);
    object(out, &[c"_class", c"messages", c"count"], &mut b);
    b.release_all();
    release(&mut arr);
    Ok(())
}

fn message_value(out: &mut CodeValue, m: &Sent) {
    let mut b = SlotBuffer::new(7);
    borrowed_str(b.slot_mut(0), c"SentMessage");
    owned_str(b.slot_mut(1), &m.from);
    owned_str(b.slot_mut(2), &m.recipient);
    owned_str(b.slot_mut(3), &m.cc);
    owned_str(b.slot_mut(4), &m.bcc);
    owned_str(b.slot_mut(5), &m.subject);
    owned_str(b.slot_mut(6), &m.body);
    object(
        out,
        &[
            c"_class",
            c"from",
            c"recipient",
            c"cc",
            c"bcc",
            c"subject",
            c"body",
        ],
        &mut b,
    );
    b.release_all();
    let _ = m.html;
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

fn require_str<'a>(particle: &'a CodeValue, name: &str, class: &str) -> Result<&'a str, String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{class} requires a non-empty string '{name}'"))
}

fn opt(particle: &CodeValue, name: &str) -> Option<String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// A `recipient`/`cc`/`bcc` field — a string, or an array of strings —
/// flattened to a comma-separated list, the shape a test wants to assert on.
fn joined(particle: &CodeValue, name: &str) -> String {
    match find_field(particle, name) {
        Some(v) if v.tag == CodeTag::Str => read_str(v).unwrap_or("").to_string(),
        Some(v) if v.tag == CodeTag::Array => array_elems(v)
            .filter_map(read_str)
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    }
}

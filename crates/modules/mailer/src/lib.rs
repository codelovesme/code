//! The `mailer` native module — send email over SMTP, for the Code
//! programming language, written in Rust on [`code-native`] over `lettre`.
//!
//! SMTP because every provider speaks it — Gmail, Amazon SES, Postmark,
//! Azure Communication Services, a relay of your own. The euglena `mailer`
//! organelle this replaces spoke Azure's signed REST API directly; SMTP is
//! the same reach with none of the vendor lock-in.
//!
//! Handlers:
//!
//! - `Config { host, port?, username?, password?, from, tls? }` →
//!   `ConfigResult { ok }` — the setup particle: the SMTP transport. `Send`
//!   is an `Exception` until it has run.
//! - `Send { recipient, subject?, text?, html?, from?, cc?, bcc? }` →
//!   `SendResult { ok }` — one message. `recipient`/`cc`/`bcc` are a string or an
//!   array of strings. A message the server rejects is an `Exception`
//!   carrying its reply.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use code_native::*;
use lettre::message::{header::ContentType, Mailbox, MessageBuilder};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{Message, SmtpTransport, Transport};
use std::sync::Mutex;

static TRANSPORT: Mutex<Option<SmtpTransport>> = Mutex::new(None);
static FROM: Mutex<Option<String>> = Mutex::new(None);

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
    guarded(&mut *out, "mailer", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Config" => config(out, particle),
            "Send" => send(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "mailer", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// `Config { host, port?, username?, password?, from, tls? }` →
/// `ConfigResult { ok }`.
fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let host = require_str(particle, "host", "Config")?;
    let from = require_str(particle, "from", "Config")?;
    // A `from` that isn't a valid address is caught now, not on the first
    // `Send`.
    from.parse::<Mailbox>()
        .map_err(|e| format!("'from' is not an email address: {e}"))?;

    let tls_mode = find_field(particle, "tls").and_then(read_str).unwrap_or("starttls");
    let default_port = match tls_mode {
        "wrapper" => 465,
        _ => 587,
    };
    let port = match find_field(particle, "port") {
        None => default_port,
        Some(v) => {
            let n = read_number(v).ok_or("'port' must be a number")?;
            if n.fract() != 0.0 || !(1.0..=65535.0).contains(&n) {
                return Err("'port' must be a whole number in 1..=65535".to_string());
            }
            n as u16
        }
    };

    let tls = match tls_mode {
        "starttls" => Tls::Required(tls_params(host)?),
        "wrapper" => Tls::Wrapper(tls_params(host)?),
        "none" => Tls::None,
        other => {
            return Err(format!(
                "tls must be \"starttls\", \"wrapper\" or \"none\", not \"{other}\""
            ))
        }
    };

    let mut builder = if matches!(tls, Tls::Wrapper(_)) {
        SmtpTransport::relay(host).map_err(|e| format!("bad SMTP host '{host}': {e}"))?
    } else {
        SmtpTransport::builder_dangerous(host)
    }
    .port(port)
    .tls(tls);

    match (
        find_field(particle, "username").and_then(read_str),
        find_field(particle, "password").and_then(read_str),
    ) {
        (Some(u), Some(p)) => builder = builder.credentials(Credentials::new(u.into(), p.into())),
        (Some(_), None) | (None, Some(_)) => {
            return Err("SMTP auth needs both 'username' and 'password', or neither".to_string())
        }
        (None, None) => {}
    }

    *TRANSPORT.lock().unwrap_or_else(|e| e.into_inner()) = Some(builder.build());
    *FROM.lock().unwrap_or_else(|e| e.into_inner()) = Some(from.to_string());

    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"ConfigResult");
    boolean(b.slot_mut(1), true);
    object(out, &[c"_class", c"ok"], &mut b);
    b.release_all();
    Ok(())
}

fn tls_params(host: &str) -> Result<TlsParameters, String> {
    TlsParameters::new(host.to_string()).map_err(|e| format!("TLS setup for '{host}' failed: {e}"))
}

// ---------------------------------------------------------------------------
// Send
// ---------------------------------------------------------------------------

/// `Send { recipient, subject?, text?, html?, from?, cc?, bcc? }` →
/// `SendResult { ok }`.
fn send(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let guard = TRANSPORT.lock().unwrap_or_else(|e| e.into_inner());
    let transport = guard
        .as_ref()
        .ok_or("mailer has no transport — send Config { host, from } first")?;

    let from = match find_field(particle, "from").and_then(read_str) {
        Some(f) => f.to_string(),
        None => FROM
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or("mailer has no transport — send Config { host, from } first")?,
    };

    let mut builder: MessageBuilder = Message::builder()
        .from(mailbox(&from, "from")?)
        .subject(find_field(particle, "subject").and_then(read_str).unwrap_or(""));

    for addr in address_list(particle, "recipient")? {
        builder = builder.to(addr);
    }
    for addr in address_list(particle, "cc")? {
        builder = builder.cc(addr);
    }
    for addr in address_list(particle, "bcc")? {
        builder = builder.bcc(addr);
    }

    let text = find_field(particle, "text").and_then(read_str);
    let html = find_field(particle, "html").and_then(read_str);
    let message = match (text, html) {
        (_, Some(html)) => builder
            .header(ContentType::TEXT_HTML)
            .body(html.to_string()),
        (Some(text), None) => builder.body(text.to_string()),
        (None, None) => builder.body(String::new()),
    }
    .map_err(|e| format!("cannot build the message: {e}"))?;

    transport
        .send(&message)
        .map_err(|e| format!("SMTP send failed: {e}"))?;

    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"SendResult");
    boolean(b.slot_mut(1), true);
    object(out, &[c"_class", c"ok"], &mut b);
    b.release_all();
    Ok(())
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

fn mailbox(addr: &str, field: &str) -> Result<Mailbox, String> {
    addr.parse()
        .map_err(|e| format!("'{field}' has a bad address '{addr}': {e}"))
}

/// A `recipient`/`cc`/`bcc` field: a single string, an array of strings, or absent
/// (empty). `recipient` being empty is caught by `lettre` when it builds the
/// message.
fn address_list(particle: &CodeValue, field: &str) -> Result<Vec<Mailbox>, String> {
    match find_field(particle, field) {
        None => Ok(Vec::new()),
        Some(v) if v.tag == CodeTag::Str => Ok(vec![mailbox(read_str(v).unwrap_or(""), field)?]),
        Some(v) if v.tag == CodeTag::Array => array_elems(v)
            .map(|e| mailbox(read_str(e).ok_or_else(|| format!("every '{field}' address must be a string"))?, field))
            .collect(),
        Some(_) => Err(format!("'{field}' must be a string or an array of strings")),
    }
}

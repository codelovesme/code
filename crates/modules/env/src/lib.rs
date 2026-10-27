//! The `env` native module — the environment, as particles.
//!
//! Everything a program needs from outside itself and cannot write down: the
//! port to listen on, the database it talks to, the secret it signs with. A
//! program that hardcodes those runs in exactly one place, and a repository
//! that contains them has leaked them.
//!
//! - `Get { name, default? }` → `EnvResult { name, value, found }`
//! - `Require { name }` → `EnvResult` or an `Exception` when it is unset
//! - `Config { file }` → `ConfigResult { ok, file, names }` — optional: this
//!   link answers from that `.env` file instead of the process environment
//!
//! **One link, one environment (ticket 180).** Unconfigured, every link reads
//! the process environment, as it always has. A link that sent `Config` reads
//! its own file and nothing else: an application held inside a host shares
//! the host's process, and the process environment is the host's — two held
//! applications that each want their own `MAIL_FROM` could not both have it
//! there. Each link of this module is its own instance (`code_abi.h` item 13),
//! so each one's file is its own.
//!
//! ```code
//! link "env.so" as env
//! link "http_server.so" as srv
//!
//! emit Get { name = "PORT", default = 8080 } to env get p
//! emit Listen { port = p.value } to srv get l
//! ```
//!
//! **The default says how to read it.** There are no type keywords in this
//! language and this module invents none: a Number default parses the
//! variable as a number, a Bool default as a boolean, anything else (or no
//! default at all) hands back the raw string. That is what makes the example
//! above one emit instead of two — the port arrives as a Number because 8080
//! is one.
//!
//! A variable that is set but unreadable *as the default's kind* is an
//! `Exception`, not a silent fallback: `PORT=banana` is a deployment
//! mistake, and quietly listening on 8080 instead would hide it until
//! someone wondered why nothing was reaching the service.

use std::collections::HashMap;
use std::env;
use std::sync::Mutex;

use code_native::*;

/// One link's environment: `None` reads the process's, `Some` the file it was
/// configured with.
struct Instance {
    file: Mutex<Option<HashMap<String, String>>>,
}

/// The instance a host that knows no instances dispatches to: unconfigured,
/// the process environment, as before.
static DEFAULT: Instance = Instance {
    file: Mutex::new(None),
};

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
    dispatch(&DEFAULT, &mut *out, &*particle)
}

/// A new link's own environment (`code_abi.h` item 13). Never freed: a module
/// is not unloaded.
#[no_mangle]
pub extern "C" fn code_module_instance_new() -> *mut core::ffi::c_void {
    Box::into_raw(Box::new(Instance {
        file: Mutex::new(None),
    })) as *mut core::ffi::c_void
}

/// `code_module_dispatch` for one link's instance.
///
/// # Safety
///
/// `instance` must come from `code_module_instance_new`; the other two as
/// for `code_module_dispatch`.
#[no_mangle]
pub unsafe extern "C" fn code_module_instance_dispatch(
    instance: *mut core::ffi::c_void,
    out: *mut CodeValue,
    particle: *const CodeValue,
) {
    dispatch(&*(instance as *const Instance), &mut *out, &*particle)
}

fn dispatch(instance: &Instance, out: &mut CodeValue, particle: &CodeValue) {
    guarded(out, "env", |out| match read_field_str(particle, "_class") {
        Some("Config") => config(instance, out, particle),
        Some("Get") => get(instance, out, particle, false),
        Some("Require") => get(instance, out, particle, true),
        _ => null(out),
    })
}

/// `Config { file }`: from now on this link answers from `file` only.
fn config(instance: &Instance, out: &mut CodeValue, particle: &CodeValue) {
    let Some(path) = read_field_str(particle, "file").filter(|p| !p.is_empty()) else {
        exception(out, "env", "Config requires a non-empty string 'file'");
        return;
    };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            exception(out, "env", &format!("cannot read {path}: {e}"));
            return;
        }
    };
    let values = parse_dotenv(&text);
    let names = values.len();
    *instance.file.lock().unwrap_or_else(|e| e.into_inner()) = Some(values);
    let mut buf = SlotBuffer::new(4);
    borrowed_str(buf.slot_mut(0), c"ConfigResult");
    boolean(buf.slot_mut(1), true);
    owned_str(buf.slot_mut(2), path);
    number(buf.slot_mut(3), names as f64);
    object(out, &[c"_class", c"ok", c"file", c"names"], &mut buf);
    buf.release_all();
}

/// A `.env` file's `KEY=VALUE` lines, read as euglena reads them: blank lines
/// and `#` comments skipped, both sides trimmed, one pair of surrounding
/// quotes (`"…"` or `'…'`) taken off the value, a line without `=` ignored. A
/// name given twice keeps the last.
fn parse_dotenv(text: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        let quoted = value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')));
        let value = if quoted {
            &value[1..value.len() - 1]
        } else {
            value
        };
        values.insert(key.trim().to_string(), value.to_string());
    }
    values
}

/// This link's value for `name`: its file's when it was configured with one,
/// the process environment's otherwise.
fn lookup(instance: &Instance, name: &str) -> Option<String> {
    match &*instance.file.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(values) => values.get(name).cloned(),
        None => env::var(name).ok(),
    }
}

fn get(instance: &Instance, out: &mut CodeValue, particle: &CodeValue, required: bool) {
    let Some(name) = read_field_str(particle, "name").filter(|n| !n.is_empty()) else {
        exception(out, "env", "Get requires a non-empty string 'name'");
        return;
    };
    // Read before anything else is decided: whether it was *set* is the one
    // fact this module has that the program cannot get any other way, and it
    // is reported (`found`) even when a default fills the value in.
    let raw = lookup(instance, name);
    let default = find_field(particle, "default");

    let found = raw.is_some();
    let Some(text) = raw else {
        if required {
            exception(out, "env", &format!("{name} is not set"));
            return;
        }
        // Absent: the default verbatim, or null when there is none. Not an
        // Exception — asking for something that might not be there is what
        // `default` means, and `found` says which happened.
        let mut buf = SlotBuffer::new(4);
        borrowed_str(buf.slot_mut(0), c"EnvResult");
        owned_str(buf.slot_mut(1), name);
        match default {
            Some(value) => copy(buf.slot_mut(2), value),
            None => null(buf.slot_mut(2)),
        }
        boolean(buf.slot_mut(3), false);
        object(out, &[c"_class", c"name", c"value", c"found"], &mut buf);
        buf.release_all();
        return;
    };

    // Set: the default's *kind* decides how to read it. A Number default
    // wants a number; a Bool default wants a boolean; anything else, and no
    // default at all, wants the string as it stands.
    let mut buf = SlotBuffer::new(4);
    borrowed_str(buf.slot_mut(0), c"EnvResult");
    owned_str(buf.slot_mut(1), name);
    match default.map(|d| d.tag) {
        Some(CodeTag::Number) => match text.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => number(buf.slot_mut(2), n),
            _ => {
                buf.release_all();
                exception(
                    out,
                    "env",
                    &format!("{name} is not a number: '{text}' (the default is one)"),
                );
                return;
            }
        },
        Some(CodeTag::Bool) => match text.trim() {
            "true" | "1" | "yes" => boolean(buf.slot_mut(2), true),
            "false" | "0" | "no" => boolean(buf.slot_mut(2), false),
            other => {
                buf.release_all();
                exception(
                    out,
                    "env",
                    &format!(
                        "{name} is not a boolean: '{other}' (expected true/false, 1/0 or yes/no)"
                    ),
                );
                return;
            }
        },
        _ => owned_str(buf.slot_mut(2), &text),
    }
    boolean(buf.slot_mut(3), found);
    object(out, &[c"_class", c"name", c"value", c"found"], &mut buf);
    buf.release_all();
}

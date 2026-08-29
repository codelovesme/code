//! The `env` native module — the environment, as particles.
//!
//! Everything a program needs from outside itself and cannot write down: the
//! port to listen on, the database it talks to, the secret it signs with. A
//! program that hardcodes those runs in exactly one place, and a repository
//! that contains them has leaked them.
//!
//! - `Get { name, default? }` → `EnvResult { name, value, found }`
//! - `Require { name }` → `EnvResult` or an `Exception` when it is unset
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

use std::env;

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
    guarded(&mut *out, "env", |out| {
        match read_field_str(particle, "_class") {
            Some("Get") => get(out, particle, false),
            Some("Require") => get(out, particle, true),
            _ => null(out),
        }
    })
}

fn get(out: &mut CodeValue, particle: &CodeValue, required: bool) {
    let Some(name) = read_field_str(particle, "name").filter(|n| !n.is_empty()) else {
        exception(out, "env", "Get requires a non-empty string 'name'");
        return;
    };
    // Read before anything else is decided: whether it was *set* is the one
    // fact this module has that the program cannot get any other way, and it
    // is reported (`found`) even when a default fills the value in.
    let raw = env::var(name).ok();
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

//! The `strings` native module — text operations for the Code programming
//! language, written in Rust on [`code-native`].
//!
//! First first-party module to take the Rust path: `terminal` stays C
//! because it is the canonical reference implementation (zero framework
//! between a reader and the ABI), while everything whose substance is logic
//! rather than syscalls goes through the crate. That split keeps both paths
//! exercised in production — if the ABI ever drifts, a C module and Rust
//! modules fail differently, which is exactly the diagnostic you want.
//!
//! Handlers (each takes `{ "value": … }`, returns `<Name>Result`):
//!
//! - `Shout`   — uppercase ASCII letters, append `!` (parity with
//!   `test_math`'s original `Shout`; the split proposal moved it here)
//! - `Echo`    — unchanged passthrough, including nested Array/Object
//! - `Split`   — array of substrings on a single-character separator
//! - `Join`    — one string from an array of strings on a single-character
//!   separator
//! - `Trim`    — leading/trailing whitespace removed
//! - `Upper`   — ASCII uppercased
//! - `Lower`   — ASCII lowercased
//!
//! Case conversion is ASCII-only, like `test_math`'s `Shout`: the language
//! has no Unicode story yet, and pretending otherwise would be worse than
//! being explicit about the range. Whitespace is the ASCII set
//! (space/tab/newline/carriage-return/form-feed/vertical-tab).
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it (see that crate's docs for
//! why the rename-and-re-export dance exists at all).

use code_native::*;

/// The ABI version this module speaks. Must equal `CODE_ABI_VERSION` or the
/// host refuses to load us.
#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// The single dispatch point: read the particle's `_class`, route to the
/// matching handler. Unknown classes raise a fatal error naming the class —
/// the same wording shape `test_math` uses, so a mis-emitted particle points
/// at itself in both backends.
///
/// # Safety
///
/// Both pointers must be valid for reads/writes respectively for the
/// duration of the call, and refer to values laid out per `code_abi.h` —
/// the host guarantees this on every dispatch (see `native.rs`).
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    if particle.tag != CodeTag::Object {
        runtime_error("strings: emit requires a particle");
    }
    let class = match read_field_str(particle, "_class") {
        Some(c) => c,
        None => runtime_error("strings: emit requires a particle"),
    };
    let out = &mut *out;

    match class {
        "Shout" => shout(out, particle),
        "Echo" => echo(out, particle),
        "Split" => split(out, particle),
        "Join" => join(out, particle),
        "Trim" => trim(out, particle),
        "Upper" => case(out, particle, true),
        "Lower" => case(out, particle, false),
        // A class this module does not handle answers null rather than
        // ending the program — whether to act on a particle is the
        // recipient's business (2026-08-28, docs/todo/errors-as-particles.md).
        _ => null(out),
    }
}

// ---------------------------------------------------------------------------
// Shared operand extraction — every handler wants `value` off the particle,
// typed per-handler. Mirrors `test_math`'s per-field checks: a missing field
// and a wrong-typed field are different mistakes, and the errors say which.
// ---------------------------------------------------------------------------

fn require_value<'a>(particle: &'a CodeValue, class: &str) -> &'a CodeValue {
    match find_field(particle, "value") {
        Some(v) => v,
        None => runtime_error(&format!("{class} requires a 'value' field")),
    }
}

fn require_string<'a>(particle: &'a CodeValue, class: &str) -> &'a str {
    let v = require_value(particle, class);
    match read_str(v) {
        Some(s) => s,
        None => runtime_error(&format!("{class} requires a string 'value'")),
    }
}

/// A single-character separator — `Split`/`Join` refuse multi-character
/// separators outright rather than silently taking the first character.
fn require_separator(particle: &CodeValue, class: &str) -> char {
    let sep_field = match find_field(particle, "separator") {
        Some(v) => v,
        None => runtime_error(&format!("{class} requires a 'separator' field")),
    };
    let sep = match read_str(sep_field) {
        Some(s) => s,
        None => runtime_error(&format!("{class} requires a string 'separator' field")),
    };
    let mut chars = sep.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => c,
        _ => runtime_error(&format!(
            "{class} requires a single-character 'separator' field"
        )),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Uppercase ASCII letters and append `!`. Byte-for-byte parity with
/// `test_math`'s original `Shout` (which this module inherits in the split
/// proposal): non-letter bytes pass through untouched, so `"whisper"` →
/// `"WHISPER!"` but `"café"` → `"CAFÉ!"` (only the ASCII range moves).
fn shout(out: &mut CodeValue, particle: &CodeValue) {
    let s = require_string(particle, "Shout");
    let shouted: String = s
        .chars()
        .map(|c| c.to_ascii_uppercase())
        .collect::<String>()
        + "!";
    make_result(out, c"ShoutResult", |slot| owned_str(slot, &shouted));
}

/// Unchanged passthrough, including nested Array/Object — exercises a
/// handler that shares structure with its input rather than building
/// something fresh, which every other handler here does. Parity with
/// `test_math`'s `Echo`.
fn echo(out: &mut CodeValue, particle: &CodeValue) {
    let value = require_value(particle, "Echo");
    make_result(out, c"EchoResult", |slot| copy(slot, value));
}

/// Split a string on a single-character separator. Empty segments survive
/// (`"a,,b"` → `["a", "", "b"]`) and a leading/trailing separator yields an
/// empty first/last segment — the least-surprising semantics, and what
/// `split` means in every language this user has come from.
fn split(out: &mut CodeValue, particle: &CodeValue) {
    let s = require_string(particle, "Split");
    let sep = require_separator(particle, "Split");
    let parts: Vec<&str> = s.split(sep).collect();
    let mut buf = SlotBuffer::new(parts.len());
    for (i, part) in parts.iter().enumerate() {
        owned_str(buf.slot_mut(i as i64), part);
    }
    make_result(out, c"SplitResult", |slot| {
        array(slot, &mut buf);
        buf.release_all();
    });
}

/// Join an array of strings with a single-character separator. Non-string
/// elements are refused — coercing numbers into strings here would invent a
/// number-formatting policy the language hasn't decided (that belongs to a
/// future formatting story, not to `Join`).
fn join(out: &mut CodeValue, particle: &CodeValue) {
    let items = require_value(particle, "Join");
    if items.tag != CodeTag::Array {
        runtime_error("Join requires an array 'value'");
    }
    let sep = require_separator(particle, "Join");
    let parts = array_elems(items)
        .map(read_str)
        .collect::<Option<Vec<_>>>()
        .unwrap_or_else(|| runtime_error("Join requires an array of strings"));
    let mut joined = String::new();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            joined.push(sep);
        }
        joined.push_str(part);
    }
    make_result(out, c"JoinResult", |slot| owned_str(slot, &joined));
}

/// Remove leading and trailing whitespace (ASCII set). Interior whitespace
/// is untouched — `Trim` trims, it does not collapse.
fn trim(out: &mut CodeValue, particle: &CodeValue) {
    let s = require_string(particle, "Trim");
    let trimmed = s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c' | '\x0b'));
    make_result(out, c"TrimResult", |slot| owned_str(slot, trimmed));
}

/// ASCII case conversion shared by `Upper` and `Lower` — one loop, one flag,
/// no reason to write it twice.
fn case(out: &mut CodeValue, particle: &CodeValue, upper: bool) {
    let class = if upper { "Upper" } else { "Lower" };
    let s = require_string(particle, class);
    let converted: String = s
        .chars()
        .map(|c| {
            if upper {
                c.to_ascii_uppercase()
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect();
    make_result(
        out,
        if upper {
            c"UpperResult"
        } else {
            c"LowerResult"
        },
        |slot| owned_str(slot, &converted),
    );
}

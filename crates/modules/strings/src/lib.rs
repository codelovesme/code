//! The `strings` native module — text operations for the Code programming
//! language, written in Rust on [`code-native`].
//!
//! First first-party module to take the Rust path: `console` stays C
//! because it is the canonical reference implementation (zero framework
//! between a reader and the ABI), while everything whose substance is logic
//! rather than syscalls goes through the crate. That split keeps both paths
//! exercised in production — if the ABI ever drifts, a C module and Rust
//! modules fail differently, which is exactly the diagnostic you want.
//!
//! Handlers (each takes `{ value = … }`, returns `<Name>Result`):
//!
//! - `Shout`   — uppercase ASCII letters, append `!` (parity with
//!   `test_math`'s original `Shout`; the split proposal moved it here)
//! - `Echo`    — unchanged passthrough, including nested Array/Object
//! - `Split`   — array of substrings on a nonempty text separator
//! - `Join`    — one string from an array of strings on a text
//!   separator
//! - `Trim`    — leading/trailing whitespace removed
//! - `Upper`   — ASCII uppercased
//! - `Lower`   — ASCII lowercased
//! - `Length`  — Unicode scalar count
//! - `Contains` / `StartsWith` / `EndsWith` / `IndexOf` — literal search, optional ASCII folding
//! - `Replace` — literal non-overlapping replacement
//! - `Slice` — scalar-indexed, clamped range
//! - `Pad` — bounded scalar-width padding
//!
//! See README.md for fields, defaults and exact separator/index contracts.
//!
//! Case conversion is ASCII-only, like `test_math`'s `Shout`: the language
//! has no Unicode story yet, and pretending otherwise would be worse than
//! being explicit about the range. Whitespace is the ASCII set
//! (space/tab/newline/carriage-return/form-feed/vertical-tab).
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it (see that crate's docs for
//! why the rename-and-re-export dance exists at all).

#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
    use code_native::*;
    use std::borrow::Cow;

    /// The ABI version this module speaks. Must equal `CODE_ABI_VERSION` or the
    /// host refuses to load us.
    #[no_mangle]
    pub extern "C" fn code_module_abi_version() -> u32 {
        CODE_ABI_VERSION
    }

    /// The single dispatch point: read the particle's `_class`, route to the
    /// matching handler. A class this module does not handle is null; a handler
    /// that cannot do the work returns an `Exception`. Neither ends the program
    /// — see `docs/todo/errors-as-particles.md`.
    ///
    /// # Safety
    ///
    /// Both pointers must be valid for reads/writes respectively for the
    /// duration of the call, and refer to values laid out per `code_abi.h` —
    /// the host guarantees this on every dispatch (see `native.rs`).
    #[no_mangle]
    pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
        let particle = &*particle;
        // `guarded` so a panic anywhere below becomes an `Exception` rather than
        // taking the host down; the `Err` arm turns a handler's own refusal into
        // the same shape, so both kinds of failure reach the caller as a value.
        guarded(&mut *out, "strings", |out| {
            let class = read_field_str(particle, "_class").unwrap_or("");
            let outcome = match class {
                "Shout" => shout(out, particle),
                "Echo" => echo(out, particle),
                "Split" => split(out, particle),
                "Join" => join(out, particle),
                "Trim" => trim(out, particle),
                "Upper" => case(out, particle, true),
                "Lower" => case(out, particle, false),
                "Length" => length(out, particle),
                "Contains" | "StartsWith" | "EndsWith" | "IndexOf" => search(out, particle, class),
                "Replace" => replace(out, particle),
                "Slice" => slice(out, particle),
                "Pad" => pad(out, particle),
                // A class this module does not handle answers null — whether to
                // act on a particle is the recipient's business.
                _ => {
                    null(out);
                    Ok(())
                }
            };
            if let Err(message) = outcome {
                exception(out, "strings", &message);
            }
        })
    }

    // ---------------------------------------------------------------------------
    // Shared operand extraction — every handler wants `value` off the particle,
    // typed per-handler. Mirrors `test_math`'s per-field checks: a missing field
    // and a wrong-typed field are different mistakes, and the errors say which.
    // ---------------------------------------------------------------------------

    /// A field the particle does not carry is null — the same answer `.field`
    /// gives — so there is no separate "you didn't supply it" case. Emitting a
    /// particle is not a form to be validated before the handler may run
    /// (owner's rule, 2026-08-28); `net` was rewritten around this in phase 2 and
    /// this is the rest of the modules catching up.
    fn require_string<'a>(particle: &'a CodeValue, class: &str) -> Result<&'a str, String> {
        find_field(particle, "value")
            .and_then(read_str)
            .ok_or_else(|| format!("{class} requires a string 'value'"))
    }

    fn string_field<'a>(
        particle: &'a CodeValue,
        class: &str,
        field: &str,
    ) -> Result<&'a str, String> {
        find_field(particle, field)
            .and_then(read_str)
            .ok_or_else(|| format!("{class} requires a string '{field}' field"))
    }

    fn require_separator<'a>(particle: &'a CodeValue, class: &str) -> Result<&'a str, String> {
        let sep = string_field(particle, class, "separator")?;
        if class == "Split" && sep.is_empty() {
            return Err("Split requires a non-empty 'separator' field".into());
        }
        Ok(sep)
    }

    // ---------------------------------------------------------------------------
    // Handlers
    // ---------------------------------------------------------------------------

    /// Uppercase ASCII letters and append `!`. Byte-for-byte parity with
    /// `test_math`'s original `Shout` (which this module inherits in the split
    /// proposal): non-letter bytes pass through untouched, so `"whisper"` →
    /// `"WHISPER!"` but `"café"` → `"CAFé!"` (only the ASCII range moves).
    fn shout(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
        let s = require_string(particle, "Shout")?;
        let shouted: String = s
            .chars()
            .map(|c| c.to_ascii_uppercase())
            .collect::<String>()
            + "!";
        make_result(out, c"ShoutResult", |slot| owned_str(slot, &shouted));
        Ok(())
    }

    /// Unchanged passthrough, including nested Array/Object — exercises a
    /// handler that shares structure with its input rather than building
    /// something fresh, which every other handler here does. Parity with
    /// `test_math`'s `Echo`.
    fn echo(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
        // `Echo { }` echoes null: there is no value this handler cannot pass
        // through, so it has nothing to validate.
        make_result(out, c"EchoResult", |slot| {
            match find_field(particle, "value") {
                Some(value) => copy(slot, value),
                None => null(slot),
            }
        });
        Ok(())
    }

    /// Split a string on a nonempty text separator. Empty segments survive
    /// (`"a,,b"` → `["a", "", "b"]`) and a leading/trailing separator yields an
    /// empty first/last segment — the least-surprising semantics, and what
    /// `split` means in every language this user has come from.
    fn split(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
        let s = require_string(particle, "Split")?;
        let sep = require_separator(particle, "Split")?;
        let parts: Vec<&str> = s.split(sep).collect();
        let mut buf = SlotBuffer::new(parts.len());
        for (i, part) in parts.iter().enumerate() {
            owned_str(buf.slot_mut(i as i64), part);
        }
        make_result(out, c"SplitResult", |slot| {
            array(slot, &mut buf);
            buf.release_all();
        });
        Ok(())
    }

    /// Join an array of strings with a literal separator (which may be empty). Non-string
    /// elements are refused — coercing numbers into strings here would invent a
    /// number-formatting policy the language hasn't decided (that belongs to a
    /// future formatting story, not to `Join`).
    fn join(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
        let items = find_field(particle, "value")
            .filter(|v| v.tag == CodeTag::Array)
            .ok_or("Join requires an array 'value'")?;
        let sep = require_separator(particle, "Join")?;
        let parts = array_elems(items)
            .map(read_str)
            .collect::<Option<Vec<_>>>()
            .ok_or("Join requires an array of strings")?;
        let mut joined = String::new();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                joined.push_str(sep);
            }
            joined.push_str(part);
        }
        make_result(out, c"JoinResult", |slot| owned_str(slot, &joined));
        Ok(())
    }

    /// Remove leading and trailing whitespace (ASCII set). Interior whitespace
    /// is untouched — `Trim` trims, it does not collapse.
    fn trim(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
        let s = require_string(particle, "Trim")?;
        let trimmed =
            s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c' | '\x0b'));
        make_result(out, c"TrimResult", |slot| owned_str(slot, trimmed));
        Ok(())
    }

    /// ASCII case conversion shared by `Upper` and `Lower` — one loop, one flag,
    /// no reason to write it twice.
    fn case(out: &mut CodeValue, particle: &CodeValue, upper: bool) -> Result<(), String> {
        let class = if upper { "Upper" } else { "Lower" };
        let s = require_string(particle, class)?;
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
        Ok(())
    }

    fn search(out: &mut CodeValue, particle: &CodeValue, class: &str) -> Result<(), String> {
        let value = require_string(particle, class)?;
        let needle = string_field(particle, class, "search")?;
        let ignore_case = match find_field(particle, "ignore_case") {
            None => false,
            Some(v) => read_bool(v)
                .ok_or_else(|| format!("{class} requires a boolean 'ignore_case' field"))?,
        };
        let (value, needle) = if ignore_case {
            (
                Cow::Owned(value.to_ascii_lowercase()),
                Cow::Owned(needle.to_ascii_lowercase()),
            )
        } else {
            (Cow::Borrowed(value), Cow::Borrowed(needle))
        };
        match class {
            "Contains" => make_result(out, c"ContainsResult", |slot| {
                boolean(slot, value.contains(needle.as_ref()))
            }),
            "StartsWith" => make_result(out, c"StartsWithResult", |slot| {
                boolean(slot, value.starts_with(needle.as_ref()))
            }),
            "EndsWith" => make_result(out, c"EndsWithResult", |slot| {
                boolean(slot, value.ends_with(needle.as_ref()))
            }),
            _ => {
                let index = value
                    .find(needle.as_ref())
                    .map(|i| value[..i].chars().count() as f64)
                    .unwrap_or(-1.0);
                make_result(out, c"IndexOfResult", |slot| number(slot, index));
            }
        }
        Ok(())
    }

    fn replace(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
        let value = require_string(particle, "Replace")?;
        let needle = string_field(particle, "Replace", "search")?;
        let replacement = string_field(particle, "Replace", "replacement")?;
        if needle.is_empty() {
            return Err("Replace requires a non-empty 'search' field".into());
        }
        let result = value.replace(needle, replacement);
        make_result(out, c"ReplaceResult", |slot| owned_str(slot, &result));
        Ok(())
    }

    fn integer_field(
        particle: &CodeValue,
        class: &str,
        field: &str,
        default: Option<i64>,
    ) -> Result<i64, String> {
        match find_field(particle, field) {
            None if default.is_some() => Ok(default.unwrap()),
            value => value
                .and_then(read_number)
                .filter(|n| n.is_finite() && n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0)
                .map(|n| n as i64)
                .ok_or_else(|| format!("{class} requires a safe integer '{field}' field")),
        }
    }

    fn slice(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
        let value = require_string(particle, "Slice")?;
        let len = value.chars().count() as i64;
        let start = integer_field(particle, "Slice", "start", Some(0))?;
        let end = integer_field(particle, "Slice", "end", Some(len))?;
        let bound = |n: i64| if n < 0 { (len + n).max(0) } else { n.min(len) };
        let (start, end) = (bound(start), bound(end));
        let result: String = value
            .chars()
            .skip(start as usize)
            .take((end - start).max(0) as usize)
            .collect();
        make_result(out, c"SliceResult", |slot| owned_str(slot, &result));
        Ok(())
    }

    fn pad(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
        let value = require_string(particle, "Pad")?;
        let width = integer_field(particle, "Pad", "width", None)?;
        if !(0..=1_000_000).contains(&width) {
            return Err("Pad requires 'width' between 0 and 1000000".into());
        }
        let fill = match find_field(particle, "fill") {
            None => " ",
            Some(_) => string_field(particle, "Pad", "fill")?,
        };
        if fill.is_empty() {
            return Err("Pad requires a non-empty 'fill' field".into());
        }
        let side = match find_field(particle, "side") {
            None => "left",
            Some(_) => string_field(particle, "Pad", "side")?,
        };
        if side != "left" && side != "right" {
            return Err("Pad requires 'side' to be left or right".into());
        }
        let needed = (width as usize).saturating_sub(value.chars().count());
        let padding: String = fill.chars().cycle().take(needed).collect();
        let result = if side == "left" {
            padding + value
        } else {
            value.to_owned() + &padding
        };
        make_result(out, c"PadResult", |slot| owned_str(slot, &result));
        Ok(())
    }

    fn length(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
        let text = require_string(particle, "Length")?;
        make_result(out, c"LengthResult", |slot| {
            number(slot, text.chars().count() as f64)
        });
        Ok(())
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn native_integer_validation_and_maximum_padding() {
            for (class, field) in [(c"Pad", c"width"), (c"Slice", c"start"), (c"Slice", c"end")] {
                for n in [
                    f64::NAN,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    0.5,
                    9_007_199_254_740_992.0,
                ] {
                    let mut fields = SlotBuffer::new(3);
                    borrowed_str(fields.slot_mut(0), class);
                    borrowed_str(fields.slot_mut(1), c"x");
                    number(fields.slot_mut(2), n);
                    let mut particle = CodeValue::zeroed();
                    object(&mut particle, &[c"_class", c"value", field], &mut fields);
                    fields.release_all();
                    let mut answer = CodeValue::zeroed();
                    // SAFETY: initialized, owned ABI values.
                    unsafe {
                        code_module_dispatch(&mut answer, &particle);
                    }
                    assert_eq!(read_field_str(&answer, "_class"), Some("Exception"));
                    assert_eq!(read_field_str(&answer, "source"), Some("strings"));
                    assert_eq!(
                        read_field_str(&answer, "message"),
                        Some(
                            format!(
                                "{} requires a safe integer '{}' field",
                                class.to_str().unwrap(),
                                field.to_str().unwrap()
                            )
                            .as_str()
                        )
                    );
                    release(&mut answer);
                    release(&mut particle);
                }
            }
            let mut fields = SlotBuffer::new(4);
            borrowed_str(fields.slot_mut(0), c"Pad");
            borrowed_str(fields.slot_mut(1), c"😀");
            number(fields.slot_mut(2), 1_000_000.0);
            borrowed_str(fields.slot_mut(3), c"é😀");
            let mut particle = CodeValue::zeroed();
            object(
                &mut particle,
                &[c"_class", c"value", c"width", c"fill"],
                &mut fields,
            );
            fields.release_all();
            let mut answer = CodeValue::zeroed();
            // SAFETY: initialized, owned ABI values.
            unsafe {
                code_module_dispatch(&mut answer, &particle);
            }
            let value = read_field_str(&answer, "value").unwrap();
            assert_eq!(value.chars().count(), 1_000_000);
            assert!(value.ends_with('😀'));
            release(&mut answer);
            release(&mut particle);
        }
    }
}

#[cfg(target_arch = "wasm32")]
include!("../../browser_half.rs");
#[cfg(target_arch = "wasm32")]
browser_half!(
    "strings",
    strings_code_module_abi_version,
    strings_code_module_dispatch
);

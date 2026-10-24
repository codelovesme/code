//! The `regex` native module (ticket 103): text by pattern.
//!
//! The engine is Rust's `regex` crate: time linear in the text, whatever the
//! pattern, because it has no backreferences or look-around — a pattern from
//! a config file cannot hang the program. Flags go inside the pattern
//! (`(?i)` ignores case, `(?m)` lets `^`/`$` match at lines).
//!
//! - `Match { value, pattern }` → `MatchResult { value }`, whether it matches anywhere
//! - `Find { value, pattern }` → `FindResult { value }`, every match, in order:
//!   `{ text, start, end, groups, named }`
//! - `Replace { value, pattern, replacement }` → `ReplaceResult { value }`,
//!   every match replaced; `$1` and `${name}` bring a group in, `$$` is a `$`
//! - `Split { value, pattern }` → `SplitResult { value }`, the texts between matches
//!
//! Positions count Unicode scalars, as the `strings` module's do. A pattern
//! that does not compile answers an `Exception` saying why. Stateless
//! (`code_abi.h` item 12): the compiled patterns it keeps are a cache.

use code_native::*;
use regex_engine::Regex;
use std::cell::RefCell;
use std::collections::HashMap;

#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// Nothing a caller sets lives here, so every link of it may share one
/// instance (`code_abi.h` item 12, ticket 177).
#[no_mangle]
pub extern "C" fn code_module_stateless() -> core::ffi::c_int {
    1
}

/// # Safety
/// The host supplies valid initialized ABI values for the duration of dispatch.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "regex", |out| {
        let class = read_field_str(particle, "_class").unwrap_or("");
        let outcome = match class {
            "Match" => matches(out, particle),
            "Find" => find(out, particle),
            "Replace" => replace(out, particle),
            "Split" => split(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "regex", &message);
        }
    })
}

/// Patterns compiled so far on this thread. A program asks with the same few
/// patterns over and over; past `KEPT` different ones it starts again.
const KEPT: usize = 128;

thread_local! {
    static COMPILED: RefCell<HashMap<String, Regex>> = RefCell::new(HashMap::new());
}

fn compiled(pattern: &str) -> Result<Regex, String> {
    COMPILED.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(found) = cache.get(pattern) {
            return Ok(found.clone());
        }
        let made = Regex::new(pattern).map_err(|e| format!("pattern does not compile: {e}"))?;
        if cache.len() >= KEPT {
            cache.clear();
        }
        cache.insert(pattern.to_string(), made.clone());
        Ok(made)
    })
}

fn text<'a>(particle: &'a CodeValue, class: &str, name: &str) -> Result<&'a str, String> {
    find_field(particle, name)
        .and_then(read_str)
        .ok_or_else(|| format!("{class} requires a text '{name}'"))
}

/// The text and the compiled pattern every handler starts from.
fn operands<'a>(particle: &'a CodeValue, class: &str) -> Result<(&'a str, Regex), String> {
    let value = text(particle, class, "value")?;
    let pattern = compiled(text(particle, class, "pattern")?)?;
    Ok((value, pattern))
}

fn matches(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let (value, pattern) = operands(particle, "Match")?;
    let found = pattern.is_match(value);
    make_result(out, c"MatchResult", |slot| boolean(slot, found));
    Ok(())
}

/// Byte offsets into scalar positions, walking forward only: matches come in
/// order, so each is counted from where the last one was.
struct Positions<'a> {
    text: &'a str,
    byte: usize,
    scalar: usize,
}

impl Positions<'_> {
    fn at(&mut self, byte: usize) -> usize {
        if byte < self.byte {
            self.byte = 0;
            self.scalar = 0;
        }
        self.scalar += self.text[self.byte..byte].chars().count();
        self.byte = byte;
        self.scalar
    }
}

fn find(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let (value, pattern) = operands(particle, "Find")?;
    let names: Vec<(usize, &str)> = pattern
        .capture_names()
        .enumerate()
        .filter_map(|(i, name)| name.map(|n| (i, n)))
        .collect();
    let name_keys: Vec<&str> = names.iter().map(|(_, n)| *n).collect();
    let mut positions = Positions {
        text: value,
        byte: 0,
        scalar: 0,
    };
    let all: Vec<_> = pattern.captures_iter(value).collect();
    let mut found = SlotBuffer::new(all.len());
    for (i, caps) in all.iter().enumerate() {
        let whole = caps.get(0).expect("group 0 is the match");
        let mut fields = SlotBuffer::new(5);
        owned_str(fields.slot_mut(0), whole.as_str());
        let start = positions.at(whole.start());
        number(fields.slot_mut(1), start as f64);
        let end = positions.at(whole.end());
        number(fields.slot_mut(2), end as f64);
        let mut groups = SlotBuffer::new(caps.len() - 1);
        for g in 1..caps.len() {
            group(groups.slot_mut(g as i64 - 1), caps.get(g));
        }
        array(fields.slot_mut(3), &mut groups);
        groups.release_all();
        let mut named = SlotBuffer::new(names.len());
        for (j, (g, _)) in names.iter().enumerate() {
            group(named.slot_mut(j as i64), caps.get(*g));
        }
        object_dyn(fields.slot_mut(4), &name_keys, &mut named);
        named.release_all();
        object(
            found.slot_mut(i as i64),
            &[c"text", c"start", c"end", c"groups", c"named"],
            &mut fields,
        );
        fields.release_all();
    }
    make_result(out, c"FindResult", |slot| array(slot, &mut found));
    found.release_all();
    Ok(())
}

/// A group's text, or null for one the match went around (`(a)|b` on "b").
fn group(slot: &mut CodeValue, matched: Option<regex_engine::Match>) {
    match matched {
        Some(m) => owned_str(slot, m.as_str()),
        None => null(slot),
    }
}

fn replace(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let (value, pattern) = operands(particle, "Replace")?;
    let replacement = text(particle, "Replace", "replacement")?;
    let result = pattern.replace_all(value, replacement);
    make_result(out, c"ReplaceResult", |slot| owned_str(slot, &result));
    Ok(())
}

fn split(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let (value, pattern) = operands(particle, "Split")?;
    let parts: Vec<&str> = pattern.split(value).collect();
    let mut slots = SlotBuffer::new(parts.len());
    for (i, part) in parts.iter().enumerate() {
        owned_str(slots.slot_mut(i as i64), part);
    }
    make_result(out, c"SplitResult", |slot| array(slot, &mut slots));
    slots.release_all();
    Ok(())
}

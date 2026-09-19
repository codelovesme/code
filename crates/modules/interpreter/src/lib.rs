//! The `interpreter` native module — source text, loaded as a linked
//! program of its own.
//!
//! Handlers:
//!
//! - `Load { source, name? }` → `Loaded { id }`. The text is parsed and put
//!   through every check a file gets before it runs — handler cycles,
//!   undefined names — and its top-level statements are executed. Any of
//!   that failing is an `Exception { message }`: the source is bad, and
//!   this is where it is said. A `link` inside the source is refused: a
//!   program loaded here links nothing.
//! - `Send { id, particle }` → whatever the handler answered; `null` when
//!   no handler takes the class; an `Exception` when the handler failed,
//!   exactly as a runtime error answers anywhere.
//! - `Unload { id }` → `Unloaded { existed }`.
//! - `Loaded {}` → `Programs { items = [{ id, name }] }`.
//!
//! # What a loaded program is
//!
//! A linked module the base made itself, from text, while running. It has
//! its own handlers and its own top level, it sees nothing of the base's,
//! and it is talked to with particles — the standing a `.so` has, without
//! the file. What it cannot do is reach back: it links no modules, so the
//! world it knows is what each particle brings it. That is the point for
//! rules, plugins, and anything else a person may write into a running
//! program: the base decides what goes in and what to do with what comes
//! out.
//!
//! # Where it works
//!
//! A machine. The page has the interpreter already (`crates/code-wasm`).
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use code::interpreter::{ask_program, prepare, Environment};
use code::loader::{self, NoModules};
use code::value::Value;
use code_native::*;

/// The most source one `Load` takes. A rule is a screenful; a megabyte is
/// a mistake, and a mistake should be refused before it is parsed.
const SOURCE_AT_MOST: usize = 256 * 1024;

struct Loaded {
    name: String,
    /// Boxed so the environment never moves once made — the same rule
    /// `interpreter::run_with` keeps, and for the same reason.
    env: Box<Environment>,
}

thread_local! {
    // A program's environment holds `Rc`s and is talked to on the one
    // thread that dispatches into this module, so the table is that
    // thread's own. Ids count up and are never reused.
    static PROGRAMS: RefCell<BTreeMap<u64, Loaded>> = const { RefCell::new(BTreeMap::new()) };
    static NEXT_ID: RefCell<u64> = const { RefCell::new(1) };
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
    guarded(&mut *out, "interpreter", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Load" => load(out, particle),
            "Send" => send(out, particle),
            "Unload" => unload(out, particle),
            "Loaded" => loaded(out),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "interpreter", &message);
        }
    })
}

fn load(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let source = read_field_str(particle, "source")
        .filter(|s| !s.trim().is_empty())
        .ok_or("Load requires a non-empty string 'source'")?;
    if source.len() > SOURCE_AT_MOST {
        return Err(format!(
            "Load refuses {} bytes of source — the most it takes is {SOURCE_AT_MOST}",
            source.len()
        ));
    }
    let name = read_field_str(particle, "name").unwrap_or("").to_string();
    let identity = if name.is_empty() {
        "<loaded>".to_string()
    } else {
        format!("<{name}>")
    };
    let resolver = NoModules {
        entry_identity: identity.clone(),
        entry_text: source.to_string(),
    };
    let program = loader::load(&identity, &resolver)?;
    let env = Box::new(Environment::default());
    prepare(&program, &env)?;
    let id = NEXT_ID.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n += 1;
        id
    });
    PROGRAMS.with(|p| p.borrow_mut().insert(id, Loaded { name, env }));
    one_number(out, c"Loaded", c"id", id as f64);
    Ok(())
}

fn send(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let id = read_field_number(particle, "id")
        .filter(|n| *n >= 1.0 && n.fract() == 0.0)
        .ok_or("Send requires a number 'id' from Load")? as u64;
    let sent = find_field(particle, "particle").ok_or("Send requires a 'particle'")?;
    if sent.tag != CodeTag::Object {
        return Err("Send's 'particle' must be an object with a '_class'".to_string());
    }
    let value = to_value(sent);
    // The environment is taken out of the table for the call: a handler
    // cannot reach this module (a loaded program links nothing), so nothing
    // can ask for it meanwhile — but a borrow held across the dispatch would
    // turn a future mistake into a panic instead of an answer.
    let taken = PROGRAMS.with(|p| p.borrow_mut().remove(&id));
    let Some(loaded) = taken else {
        return Err(format!("no program is loaded as {id}"));
    };
    let answer = ask_program(&value, &loaded.env);
    PROGRAMS.with(|p| p.borrow_mut().insert(id, loaded));
    from_value(out, &answer);
    Ok(())
}

fn unload(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let id = read_field_number(particle, "id")
        .filter(|n| *n >= 1.0 && n.fract() == 0.0)
        .ok_or("Unload requires a number 'id' from Load")? as u64;
    let existed = PROGRAMS.with(|p| p.borrow_mut().remove(&id).is_some());
    let mut fields = SlotBuffer::new(2);
    borrowed_str(fields.slot_mut(0), c"Unloaded");
    boolean(fields.slot_mut(1), existed);
    object(out, &[c"_class", c"existed"], &mut fields);
    fields.release_all();
    Ok(())
}

fn loaded(out: &mut CodeValue) -> Result<(), String> {
    let items: Vec<(u64, String)> = PROGRAMS.with(|p| {
        p.borrow()
            .iter()
            .map(|(id, l)| (*id, l.name.clone()))
            .collect()
    });
    let mut elems = SlotBuffer::new(items.len());
    for (i, (id, name)) in items.iter().enumerate() {
        let mut fields = SlotBuffer::new(2);
        number(fields.slot_mut(0), *id as f64);
        owned_str(fields.slot_mut(1), name);
        object(elems.slot_mut(i as i64), &[c"id", c"name"], &mut fields);
        fields.release_all();
    }
    let mut fields = SlotBuffer::new(2);
    borrowed_str(fields.slot_mut(0), c"Programs");
    array(fields.slot_mut(1), &mut elems);
    elems.release_all();
    object(out, &[c"_class", c"items"], &mut fields);
    fields.release_all();
    Ok(())
}

fn one_number(
    out: &mut CodeValue,
    class: &'static std::ffi::CStr,
    key: &'static std::ffi::CStr,
    n: f64,
) {
    let mut fields = SlotBuffer::new(2);
    borrowed_str(fields.slot_mut(0), class);
    number(fields.slot_mut(1), n);
    object(out, &[c"_class", key], &mut fields);
    fields.release_all();
}

// ---------------------------------------------------------------------------
// Across the boundary: the module's values and the interpreter's.
//
// A particle arrives as a `CodeValue` built by the base's runtime and goes
// into the loaded program as the interpreter's own `Value`; the answer
// comes back the other way. Nothing is shared: both directions copy.
// ---------------------------------------------------------------------------

fn to_value(v: &CodeValue) -> Value {
    match v.tag {
        CodeTag::Number => Value::Number(v.number),
        CodeTag::Str => Value::Str(Rc::from(read_str(v).unwrap_or(""))),
        CodeTag::Bool => Value::Bool(v.boolean != 0),
        CodeTag::Null => Value::Null,
        CodeTag::Array => Value::Array(Rc::new(array_elems(v).map(to_value).collect())),
        CodeTag::Object => Value::Object(Rc::new(
            object_entries(v)
                .map(|(k, item)| (k.to_string(), to_value(item)))
                .collect(),
        )),
    }
}

fn from_value(out: &mut CodeValue, v: &Value) {
    match v {
        Value::Number(n) => number(out, *n),
        Value::Str(s) => owned_str(out, s),
        Value::Bool(b) => boolean(out, *b),
        Value::Null => null(out),
        Value::Array(items) => {
            let mut elems = SlotBuffer::new(items.len());
            for (i, item) in items.iter().enumerate() {
                from_value(elems.slot_mut(i as i64), item);
            }
            array(out, &mut elems);
            elems.release_all();
        }
        Value::Object(fields) => {
            let keys: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
            let mut values = SlotBuffer::new(fields.len());
            for (i, (_, item)) in fields.iter().enumerate() {
                from_value(values.slot_mut(i as i64), item);
            }
            object_dyn(out, &keys, &mut values);
            values.release_all();
        }
    }
}

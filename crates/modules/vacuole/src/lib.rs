//! The `vacuole` native module — an in-process key→value store with a byte
//! budget, for the Code programming language, written in Rust on
//! [`code-native`].
//!
//! The native counterpart of the browser-only Vacuole organelle (a WASM
//! module over a JavaScript map): the same Get/Set/Remove contract, held in
//! the memory of the process that links it, plus what a long-running
//! server needs from a cache that must stay bounded:
//!
//! - `Config { budget_bytes }` → `ConfigResult { ok, budget_bytes, evicted }`
//!   — the budget for everything this link holds (default 64 MiB; `0`
//!   turns the store off: every `Set` is refused, every `Get` misses).
//!   Lowering it evicts at once.
//! - `Set { key, value }` → `SetResult { ok, bytes, evicted }` — `evicted`
//!   names every key that had to go to make room (least recently used
//!   first), so a caller keeping an index over these keys can drop them
//!   too. A value larger than the whole budget is refused
//!   (`ok = false`, `reason`), never stored over the bound.
//! - `Get { key }` → `GetResult { found, value }`.
//! - `GetMany { keys }` → `GetManyResult { items = [{ key, found, value }] }`
//!   — at most 1000 keys.
//! - `Remove { key }` → `RemoveResult { removed }`.
//! - `RemovePrefix { prefix }` → `RemovePrefixResult { removed }` (a count).
//! - `Keys { prefix?, limit? }` → `KeysResult { items }` — sorted; does not
//!   count as a use.
//! - `Stats` → `StatsResult { count, bytes, budget_bytes, evictions, hits,
//!   misses }`.
//!
//! **What is stored.** A value is kept as its JSON (the same rules as the
//! `json` module: `_class` is dropped, whole numbers stay whole), so what
//! `Get` answers is a copy: changing it changes nothing stored. The size
//! counted is the key, the JSON text and a fixed 64 bytes per entry; it is a
//! bound on what this store holds, not on the process's memory. Keys are
//! non-empty text of at most 512 bytes.
//!
//! **Eviction is not deletion.** Nothing evicted was ever durable here: a
//! caller that needs the value again fetches it from wherever it came from.
//!
//! Each `link` has its own store (`code_abi.h` item 13); a host that knows
//! no instances shares one default store.

use std::collections::BTreeMap;
use std::sync::Mutex;

use code_native::*;
use serde_json::Value as Json;

const DEFAULT_BUDGET: usize = 64 * 1024 * 1024;
const ENTRY_OVERHEAD: usize = 64;
const MAX_KEY: usize = 512;
const MAX_MANY: usize = 1000;

struct Entry {
    value: Json,
    size: usize,
    tick: u64,
}

struct Store {
    budget: usize,
    bytes: usize,
    tick: u64,
    map: BTreeMap<String, Entry>,
    order: BTreeMap<u64, String>,
    evictions: u64,
    hits: u64,
    misses: u64,
}

impl Store {
    const fn new() -> Self {
        Store {
            budget: DEFAULT_BUDGET,
            bytes: 0,
            tick: 0,
            map: BTreeMap::new(),
            order: BTreeMap::new(),
            evictions: 0,
            hits: 0,
            misses: 0,
        }
    }

    fn touch(&mut self, key: &str) {
        self.tick += 1;
        let tick = self.tick;
        if let Some(entry) = self.map.get_mut(key) {
            self.order.remove(&entry.tick);
            entry.tick = tick;
            self.order.insert(tick, key.to_owned());
        }
    }

    fn remove(&mut self, key: &str) -> bool {
        match self.map.remove(key) {
            Some(entry) => {
                self.order.remove(&entry.tick);
                self.bytes -= entry.size;
                true
            }
            None => false,
        }
    }

    /// Least recently used first, until `extra` more bytes fit.
    fn make_room(&mut self, extra: usize) -> Vec<String> {
        let mut evicted = Vec::new();
        while self.bytes + extra > self.budget {
            let Some((_, key)) = self.order.pop_first() else {
                break;
            };
            if let Some(entry) = self.map.remove(&key) {
                self.bytes -= entry.size;
            }
            self.evictions += 1;
            evicted.push(key);
        }
        evicted
    }
}

struct Instance {
    store: Mutex<Store>,
}

static DEFAULT: Instance = Instance {
    store: Mutex::new(Store::new()),
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

/// A new link's own store (`code_abi.h` item 13). Never freed: a module is
/// not unloaded.
#[no_mangle]
pub extern "C" fn code_module_instance_new() -> *mut core::ffi::c_void {
    Box::into_raw(Box::new(Instance {
        store: Mutex::new(Store::new()),
    })) as *mut core::ffi::c_void
}

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
    guarded(out, "vacuole", |out| {
        let mut store = instance.store.lock().unwrap_or_else(|e| e.into_inner());
        let result = match read_field_str(particle, "_class") {
            Some("Config") => config(&mut store, out, particle),
            Some("Set") => set(&mut store, out, particle),
            Some("Get") => get(&mut store, out, particle),
            Some("GetMany") => get_many(&mut store, out, particle),
            Some("Remove") => remove(&mut store, out, particle),
            Some("RemovePrefix") => remove_prefix(&mut store, out, particle),
            Some("Keys") => keys(&store, out, particle),
            Some("Stats") => {
                stats(&store, out);
                Ok(())
            }
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = result {
            exception(out, "vacuole", &message);
        }
    })
}

fn key_of<'a>(particle: &'a CodeValue, class: &str) -> Result<&'a str, String> {
    let key = read_field_str(particle, "key").ok_or(format!("{class} requires a string 'key'"))?;
    if key.is_empty() || key.len() > MAX_KEY {
        return Err(format!("{class} requires a key of 1 to {MAX_KEY} bytes"));
    }
    Ok(key)
}

fn config(store: &mut Store, out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    if let Some(v) = find_field(particle, "budget_bytes") {
        let n = read_number(v).ok_or("Config requires a number 'budget_bytes'")?;
        if !n.is_finite() || n < 0.0 || n.fract() != 0.0 {
            return Err("Config requires a whole, non-negative 'budget_bytes'".into());
        }
        store.budget = n as usize;
    }
    let evicted = store.make_room(0);
    let mut buf = SlotBuffer::new(4);
    owned_str(buf.slot_mut(0), "ConfigResult");
    boolean(buf.slot_mut(1), true);
    number(buf.slot_mut(2), store.budget as f64);
    strings(buf.slot_mut(3), &evicted);
    object_dyn(out, &["_class", "ok", "budget_bytes", "evicted"], &mut buf);
    buf.release_all();
    Ok(())
}

fn set(store: &mut Store, out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = key_of(particle, "Set")?.to_owned();
    let value = find_field(particle, "value")
        .map(to_json)
        .unwrap_or(Json::Null);
    let size = key.len() + value.to_string().len() + ENTRY_OVERHEAD;
    let refused = if store.budget == 0 {
        Some("the store is off (budget 0)")
    } else if size > store.budget {
        Some("the value is larger than the whole budget")
    } else {
        None
    };
    if let Some(reason) = refused {
        let mut buf = SlotBuffer::new(5);
        owned_str(buf.slot_mut(0), "SetResult");
        boolean(buf.slot_mut(1), false);
        number(buf.slot_mut(2), size as f64);
        strings(buf.slot_mut(3), &[]);
        owned_str(buf.slot_mut(4), reason);
        object_dyn(
            out,
            &["_class", "ok", "bytes", "evicted", "reason"],
            &mut buf,
        );
        buf.release_all();
        return Ok(());
    }
    store.remove(&key);
    let evicted = store.make_room(size);
    store.tick += 1;
    let tick = store.tick;
    store.order.insert(tick, key.clone());
    store.map.insert(key, Entry { value, size, tick });
    store.bytes += size;
    let mut buf = SlotBuffer::new(4);
    owned_str(buf.slot_mut(0), "SetResult");
    boolean(buf.slot_mut(1), true);
    number(buf.slot_mut(2), size as f64);
    strings(buf.slot_mut(3), &evicted);
    object_dyn(out, &["_class", "ok", "bytes", "evicted"], &mut buf);
    buf.release_all();
    Ok(())
}

fn get(store: &mut Store, out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = key_of(particle, "Get")?;
    let found = store.map.contains_key(key);
    let mut buf = SlotBuffer::new(3);
    owned_str(buf.slot_mut(0), "GetResult");
    boolean(buf.slot_mut(1), found);
    if found {
        store.hits += 1;
        store.touch(key);
        from_json(buf.slot_mut(2), &store.map[key].value);
    } else {
        store.misses += 1;
        null(buf.slot_mut(2));
    }
    object_dyn(out, &["_class", "found", "value"], &mut buf);
    buf.release_all();
    Ok(())
}

fn get_many(store: &mut Store, out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let keys = find_field(particle, "keys")
        .filter(|v| v.tag == CodeTag::Array)
        .ok_or("GetMany requires an array 'keys'")?;
    let wanted: Vec<String> = array_elems(keys)
        .filter_map(|k| read_str(k).map(str::to_owned))
        .collect();
    if wanted.len() > MAX_MANY {
        return Err(format!("GetMany takes at most {MAX_MANY} keys"));
    }
    let mut items = SlotBuffer::new(wanted.len());
    for (i, key) in wanted.iter().enumerate() {
        let found = store.map.contains_key(key.as_str());
        let mut item = SlotBuffer::new(3);
        owned_str(item.slot_mut(0), key);
        boolean(item.slot_mut(1), found);
        if found {
            store.hits += 1;
            store.touch(key);
            from_json(item.slot_mut(2), &store.map[key.as_str()].value);
        } else {
            store.misses += 1;
            null(item.slot_mut(2));
        }
        object_dyn(
            items.slot_mut(i as i64),
            &["key", "found", "value"],
            &mut item,
        );
        item.release_all();
    }
    let mut buf = SlotBuffer::new(2);
    owned_str(buf.slot_mut(0), "GetManyResult");
    array(buf.slot_mut(1), &mut items);
    items.release_all();
    object_dyn(out, &["_class", "items"], &mut buf);
    buf.release_all();
    Ok(())
}

fn remove(store: &mut Store, out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = key_of(particle, "Remove")?;
    let removed = store.remove(key);
    let mut buf = SlotBuffer::new(2);
    owned_str(buf.slot_mut(0), "RemoveResult");
    boolean(buf.slot_mut(1), removed);
    object_dyn(out, &["_class", "removed"], &mut buf);
    buf.release_all();
    Ok(())
}

fn remove_prefix(
    store: &mut Store,
    out: &mut CodeValue,
    particle: &CodeValue,
) -> Result<(), String> {
    let prefix =
        read_field_str(particle, "prefix").ok_or("RemovePrefix requires a string 'prefix'")?;
    if prefix.is_empty() {
        return Err("RemovePrefix requires a non-empty 'prefix' (Config { budget_bytes = 0 } empties everything)".into());
    }
    let doomed: Vec<String> = store
        .map
        .keys()
        .filter(|k| k.starts_with(prefix))
        .cloned()
        .collect();
    for key in &doomed {
        store.remove(key);
    }
    let mut buf = SlotBuffer::new(2);
    owned_str(buf.slot_mut(0), "RemovePrefixResult");
    number(buf.slot_mut(1), doomed.len() as f64);
    object_dyn(out, &["_class", "removed"], &mut buf);
    buf.release_all();
    Ok(())
}

fn keys(store: &Store, out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let prefix = read_field_str(particle, "prefix").unwrap_or("");
    let limit = match find_field(particle, "limit") {
        None => usize::MAX,
        Some(v) => read_number(v)
            .filter(|n| *n >= 0.0)
            .ok_or("Keys requires a non-negative 'limit'")? as usize,
    };
    let mut found: Vec<String> = store
        .map
        .keys()
        .filter(|k| k.starts_with(prefix))
        .cloned()
        .collect();
    found.sort();
    found.truncate(limit);
    let mut buf = SlotBuffer::new(2);
    owned_str(buf.slot_mut(0), "KeysResult");
    strings(buf.slot_mut(1), &found);
    object_dyn(out, &["_class", "items"], &mut buf);
    buf.release_all();
    Ok(())
}

fn stats(store: &Store, out: &mut CodeValue) {
    let mut buf = SlotBuffer::new(7);
    owned_str(buf.slot_mut(0), "StatsResult");
    number(buf.slot_mut(1), store.map.len() as f64);
    number(buf.slot_mut(2), store.bytes as f64);
    number(buf.slot_mut(3), store.budget as f64);
    number(buf.slot_mut(4), store.evictions as f64);
    number(buf.slot_mut(5), store.hits as f64);
    number(buf.slot_mut(6), store.misses as f64);
    object_dyn(
        out,
        &[
            "_class",
            "count",
            "bytes",
            "budget_bytes",
            "evictions",
            "hits",
            "misses",
        ],
        &mut buf,
    );
    buf.release_all();
}

fn strings(out: &mut CodeValue, items: &[String]) {
    let mut buf = SlotBuffer::new(items.len());
    for (i, item) in items.iter().enumerate() {
        owned_str(buf.slot_mut(i as i64), item);
    }
    array(out, &mut buf);
    buf.release_all();
}

// ---------------------------------------------------------------------------
// CodeValue <-> serde_json (the `json` module's rules)
// ---------------------------------------------------------------------------

fn to_json(v: &CodeValue) -> Json {
    match v.tag {
        CodeTag::Number => number_to_json(v.number()),
        CodeTag::Str => Json::String(read_str(v).unwrap_or_default().to_owned()),
        CodeTag::Bool => Json::Bool(read_bool(v).unwrap_or(false)),
        CodeTag::Null => Json::Null,
        CodeTag::Array => Json::Array(array_elems(v).map(to_json).collect()),
        CodeTag::Object => {
            let mut map = serde_json::Map::new();
            for (key, value) in object_entries(v) {
                if key != "_class" {
                    map.insert(key.to_owned(), to_json(value));
                }
            }
            Json::Object(map)
        }
    }
}

fn from_json(out: &mut CodeValue, v: &Json) {
    match v {
        Json::Null => null(out),
        Json::Bool(b) => boolean(out, *b),
        Json::Number(n) => number(out, n.as_f64().unwrap_or(0.0)),
        Json::String(s) => owned_str(out, s),
        Json::Array(items) => {
            let mut buf = SlotBuffer::new(items.len());
            for (i, item) in items.iter().enumerate() {
                from_json(buf.slot_mut(i as i64), item);
            }
            array(out, &mut buf);
            buf.release_all();
        }
        Json::Object(map) => {
            let keys: Vec<&str> = map.keys().map(String::as_str).collect();
            let mut buf = SlotBuffer::new(map.len());
            for (i, value) in map.values().enumerate() {
                from_json(buf.slot_mut(i as i64), value);
            }
            object_dyn(out, &keys, &mut buf);
            buf.release_all();
        }
    }
}

fn number_to_json(n: f64) -> Json {
    if !n.is_finite() {
        Json::Null
    } else if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
        Json::Number((n as i64).into())
    } else {
        serde_json::Number::from_f64(n).map_or(Json::Null, Json::Number)
    }
}

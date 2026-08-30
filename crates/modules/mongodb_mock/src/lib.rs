//! The `mongodb_mock` native module — a drop-in for `mongodb` backed by
//! in-memory collections, for the Code programming language, written in Rust
//! on [`code-native`].
//!
//! Same particles and result shapes as `mongodb` — `Config { url, database }`
//! → `ConfigResult { ok }`, the key/value trio (`Store`/`Fetch`/`Delete`),
//! and the document operations (`Insert`, `InsertMany`, `Find`, `Count`,
//! `Drop`) — over a `HashMap` of collections that lives for the process.
//!
//! `Find` supports the filter shapes the euglena apps use: exact-match on
//! any field, `sort` by one key, `limit`, `skip`. No aggregation, no
//! operators (`$gt`, …) — the same subset the real module ships, minus the
//! server.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use code_native::*;
use serde_json::{Map, Value as Json};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

const KV_COLLECTION: &str = "state";

type Db = BTreeMap<String, Vec<Map<String, Json>>>;

static DB: Mutex<Option<Db>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

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
    guarded(&mut *out, "mongodb_mock", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Config" => config(out, particle),
            "Store" => store(out, particle),
            "Fetch" => fetch(out, particle),
            "Delete" => delete(out, particle),
            "Insert" => insert(out, particle),
            "InsertMany" => insert_many(out, particle),
            "Find" => find(out, particle),
            "Count" => count(out, particle),
            "Drop" => drop_collection(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "mongodb_mock", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Setup + key/value
// ---------------------------------------------------------------------------

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    require_str(particle, "url", "Config")?;
    require_str(particle, "database", "Config")?;
    *DB.lock().unwrap_or_else(|e| e.into_inner()) = Some(BTreeMap::new());
    one_bool(out, c"ConfigResult", c"ok", true);
    Ok(())
}

fn store(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Store")?.to_string();
    let value = find_field(particle, "value").map_or(Json::Null, to_json);
    let coll = kv_collection(particle);

    with_db(|db| {
        let docs = db.entry(coll).or_default();
        docs.retain(|d| d.get("_id").and_then(Json::as_str) != Some(key.as_str()));
        let mut doc = Map::new();
        doc.insert("_id".into(), Json::String(key.clone()));
        doc.insert("value".into(), value);
        docs.push(doc);
    })?;

    one_str(out, c"StoreResult", c"key", &key);
    Ok(())
}

fn fetch(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Fetch")?.to_string();
    let coll = kv_collection(particle);
    let value = with_db(|db| {
        db.get(&coll).and_then(|docs| {
            docs.iter()
                .find(|d| d.get("_id").and_then(Json::as_str) == Some(key.as_str()))
                .and_then(|d| d.get("value").cloned())
        })
    })?;

    let mut b = SlotBuffer::new(4);
    borrowed_str(b.slot_mut(0), c"FetchResult");
    boolean(b.slot_mut(1), value.is_some());
    owned_str(b.slot_mut(2), &key);
    match &value {
        Some(v) => from_json(b.slot_mut(3), v),
        None => null(b.slot_mut(3)),
    }
    object(out, &[c"_class", c"found", c"key", c"value"], &mut b);
    b.release_all();
    Ok(())
}

fn delete(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Delete")?.to_string();
    let coll = kv_collection(particle);
    let existed = with_db(|db| {
        db.get_mut(&coll)
            .map(|docs| {
                let before = docs.len();
                docs.retain(|d| d.get("_id").and_then(Json::as_str) != Some(key.as_str()));
                docs.len() != before
            })
            .unwrap_or(false)
    })?;
    one_bool(out, c"DeleteResult", c"existed", existed);
    Ok(())
}

fn kv_collection(particle: &CodeValue) -> String {
    find_field(particle, "collection")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(KV_COLLECTION)
        .to_string()
}

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

fn insert(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let coll = require_str(particle, "collection", "Insert")?.to_string();
    let mut doc = require_object(particle, "doc", "Insert")?;
    let id = ensure_id(&mut doc);
    with_db(|db| db.entry(coll).or_default().push(doc))?;
    one_str(out, c"InsertResult", c"id", &id);
    Ok(())
}

fn insert_many(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let coll = require_str(particle, "collection", "InsertMany")?.to_string();
    let arr = find_field(particle, "docs")
        .filter(|v| v.tag == CodeTag::Array)
        .ok_or("InsertMany requires an array 'docs'")?;
    let mut docs = Vec::new();
    for v in array_elems(arr) {
        let mut doc = object_of(v).ok_or("every element of 'docs' must be an object")?;
        ensure_id(&mut doc);
        docs.push(doc);
    }
    let n = docs.len();
    with_db(|db| db.entry(coll).or_default().extend(docs))?;
    one_number(out, c"InsertManyResult", c"count", n as f64);
    Ok(())
}

fn find(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let coll = require_str(particle, "collection", "Find")?.to_string();
    let filter = optional_object(particle, "filter")?;
    let sort = optional_object(particle, "sort")?;
    let limit = whole_number(particle, "limit")?;
    let skip = whole_number(particle, "skip")?.unwrap_or(0).max(0) as usize;

    let mut docs: Vec<Map<String, Json>> = with_db(|db| {
        db.get(&coll)
            .map(|docs| {
                docs.iter()
                    .filter(|d| matches_filter(d, &filter))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    })?;

    if let Some((key, dir)) = sort.as_ref().and_then(|s| s.iter().next()) {
        let ascending = dir.as_f64().unwrap_or(1.0) >= 0.0;
        docs.sort_by(|a, b| {
            let ord = json_cmp(a.get(key), b.get(key));
            if ascending {
                ord
            } else {
                ord.reverse()
            }
        });
    }

    let windowed: Vec<&Map<String, Json>> = docs
        .iter()
        .skip(skip)
        .take(limit.map(|n| n.max(0) as usize).unwrap_or(usize::MAX))
        .collect();

    let mut items = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(windowed.len());
    for (i, d) in windowed.iter().enumerate() {
        from_json(buf.slot_mut(i as i64), &Json::Object((*d).clone()));
    }
    array(&mut items, &mut buf);
    buf.release_all();

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"FindResult");
    copy(b.slot_mut(1), &items);
    number(b.slot_mut(2), windowed.len() as f64);
    object(out, &[c"_class", c"items", c"count"], &mut b);
    b.release_all();
    release(&mut items);
    Ok(())
}

fn count(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let coll = require_str(particle, "collection", "Count")?.to_string();
    let filter = optional_object(particle, "filter")?;
    let n = with_db(|db| {
        db.get(&coll)
            .map(|docs| docs.iter().filter(|d| matches_filter(d, &filter)).count())
            .unwrap_or(0)
    })?;
    one_number(out, c"CountResult", c"count", n as f64);
    Ok(())
}

fn drop_collection(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let coll = require_str(particle, "collection", "Drop")?.to_string();
    let existed = with_db(|db| db.remove(&coll).is_some())?;
    one_bool(out, c"DropResult", c"dropped", existed);
    Ok(())
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

fn matches_filter(doc: &Map<String, Json>, filter: &Option<Map<String, Json>>) -> bool {
    let Some(filter) = filter else { return true };
    filter
        .iter()
        .all(|(k, want)| doc.get(k).map(|got| got == want).unwrap_or(false))
}

fn json_cmp(a: Option<&Json>, b: Option<&Json>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Some(Json::Number(x)), Some(Json::Number(y))) => x
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&y.as_f64().unwrap_or(0.0))
            .unwrap_or(Ordering::Equal),
        (Some(Json::String(x)), Some(Json::String(y))) => x.cmp(y),
        (Some(Json::Bool(x)), Some(Json::Bool(y))) => x.cmp(y),
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

fn ensure_id(doc: &mut Map<String, Json>) -> String {
    match doc.get("_id").and_then(Json::as_str) {
        Some(id) => id.to_string(),
        None => {
            let id = format!("mock{:016x}", NEXT_ID.fetch_add(1, Ordering::SeqCst));
            doc.insert("_id".into(), Json::String(id.clone()));
            id
        }
    }
}

// ---------------------------------------------------------------------------
// CodeValue <-> serde_json (matches the `json` module, `_class` dropped)
// ---------------------------------------------------------------------------

fn to_json(v: &CodeValue) -> Json {
    match v.tag {
        CodeTag::Number => {
            let n = v.number;
            if n.fract() == 0.0 && n.abs() < 9e15 {
                Json::from(n as i64)
            } else {
                serde_json::Number::from_f64(n)
                    .map(Json::Number)
                    .unwrap_or(Json::Null)
            }
        }
        CodeTag::Str => Json::String(read_str(v).unwrap_or_default().to_owned()),
        CodeTag::Bool => Json::Bool(read_bool(v).unwrap_or(false)),
        CodeTag::Null => Json::Null,
        CodeTag::Array => Json::Array(array_elems(v).map(to_json).collect()),
        CodeTag::Object => {
            let mut map = Map::new();
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

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

const NOT_CONFIGURED: &str = "mongodb_mock has no database — send Config { url, database } first";

fn with_db<T>(f: impl FnOnce(&mut Db) -> T) -> Result<T, String> {
    let mut guard = DB.lock().unwrap_or_else(|e| e.into_inner());
    let db = guard.as_mut().ok_or(NOT_CONFIGURED)?;
    Ok(f(db))
}

fn object_of(v: &CodeValue) -> Option<Map<String, Json>> {
    match to_json(v) {
        Json::Object(m) => Some(m),
        _ => None,
    }
}

fn require_object(
    particle: &CodeValue,
    name: &str,
    class: &str,
) -> Result<Map<String, Json>, String> {
    find_field(particle, name)
        .and_then(object_of)
        .ok_or_else(|| format!("{class} requires an object '{name}'"))
}

fn optional_object(particle: &CodeValue, name: &str) -> Result<Option<Map<String, Json>>, String> {
    match find_field(particle, name) {
        None => Ok(None),
        Some(v) if v.tag == CodeTag::Object => Ok(object_of(v)),
        Some(_) => Err(format!("'{name}' must be an object")),
    }
}

fn whole_number(particle: &CodeValue, name: &str) -> Result<Option<i64>, String> {
    match find_field(particle, name).and_then(read_number) {
        None => Ok(None),
        Some(n) if n.fract() == 0.0 => Ok(Some(n as i64)),
        Some(_) => Err(format!("'{name}' must be a whole number")),
    }
}

fn require_str<'a>(particle: &'a CodeValue, name: &str, class: &str) -> Result<&'a str, String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{class} requires a non-empty string '{name}'"))
}

fn one_str(
    out: &mut CodeValue,
    class: &'static std::ffi::CStr,
    key: &'static std::ffi::CStr,
    value: &str,
) {
    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), class);
    owned_str(b.slot_mut(1), value);
    object(out, &[c"_class", key], &mut b);
    b.release_all();
}

fn one_bool(
    out: &mut CodeValue,
    class: &'static std::ffi::CStr,
    key: &'static std::ffi::CStr,
    value: bool,
) {
    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), class);
    boolean(b.slot_mut(1), value);
    object(out, &[c"_class", key], &mut b);
    b.release_all();
}

fn one_number(
    out: &mut CodeValue,
    class: &'static std::ffi::CStr,
    key: &'static std::ffi::CStr,
    value: f64,
) {
    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), class);
    number(b.slot_mut(1), value);
    object(out, &[c"_class", key], &mut b);
    b.release_all();
}

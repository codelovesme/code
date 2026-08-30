//! The `mongodb` native module — a MongoDB collection, for the Code
//! programming language, written in Rust on [`code-native`] over the
//! official synchronous driver.
//!
//! Two layers over one connection:
//!
//! - **Documents** — `Insert`, `InsertMany`, `Find`, `Count` on a named
//!   collection. `Find` returns an array of objects, not a JSON string.
//! - **Key/value** — `Store`, `Fetch`, `Delete` by a string key, stored as
//!   `{ _id: key, value: … }` in a `state` collection (override with
//!   `collection`). The convenient case; `json_store` is the file-backed
//!   one.
//!
//! Handlers:
//!
//! - `Config { url, database }` → `ConfigResult { ok }` — connect and ping.
//!   The setup particle: everything else is an `Exception` until it runs.
//! - `Store { key, value, collection? }` → `StoreResult { key }`
//! - `Fetch { key, collection? }` → `FetchResult { found, key, value }`
//! - `Delete { key, collection? }` → `DeleteResult { existed }`
//! - `Insert { collection, doc }` → `InsertResult { id }`
//! - `InsertMany { collection, docs }` → `InsertManyResult { count }`
//! - `Find { collection, filter?, sort?, limit?, skip? }` → `FindResult { items, count }`
//! - `Count { collection, filter? }` → `CountResult { count }`
//!
//! `ObjectId` and `DateTime` come back as strings (hex, RFC 3339); every
//! other BSON type maps onto one of the language's six.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use driver::bson::{doc, Bson, Document};
use code_native::*;
use driver::sync::{Client, Collection, Database};
use std::sync::Mutex;

static DB: Mutex<Option<Database>> = Mutex::new(None);

const NOT_CONNECTED: &str = "mongodb is not connected — send Config { url, database } first";
const KV_COLLECTION: &str = "state";

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
    guarded(&mut *out, "mongodb", |out| {
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
            exception(out, "mongodb", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let url = require_str(particle, "url", "Config")?;
    let database = require_str(particle, "database", "Config")?;

    // Timeouts come from the URL (`serverSelectionTimeoutMS`,
    // `connectTimeoutMS`) or the driver's defaults — a module that clamped
    // them would be overriding a deployment decision.
    let opts = driver::options::ClientOptions::parse(url)
        .run()
        .map_err(|e| format!("bad connection URL: {e}"))?;

    let client = Client::with_options(opts).map_err(|e| format!("cannot create client: {e}"))?;
    let db = client.database(database);
    db.run_command(doc! { "ping": 1 })
        .run()
        .map_err(|e| format!("cannot reach MongoDB at '{}': {e}", redact(url)))?;

    *DB.lock().unwrap_or_else(|e| e.into_inner()) = Some(db);
    ok_result(out, c"ConfigResult");
    Ok(())
}

// ---------------------------------------------------------------------------
// Key/value
// ---------------------------------------------------------------------------

fn store(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Store")?.to_string();
    let value = find_field(particle, "value").map_or(Bson::Null, code_to_bson);
    kv_collection(particle)?
        .replace_one(doc! { "_id": &key }, doc! { "_id": &key, "value": value })
        .upsert(true)
        .run()
        .map_err(|e| format!("Store failed: {e}"))?;
    one_str(out, c"StoreResult", c"key", &key);
    Ok(())
}

fn fetch(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Fetch")?.to_string();
    let found = kv_collection(particle)?
        .find_one(doc! { "_id": &key })
        .run()
        .map_err(|e| format!("Fetch failed: {e}"))?;

    let mut b = SlotBuffer::new(4);
    borrowed_str(b.slot_mut(0), c"FetchResult");
    boolean(b.slot_mut(1), found.is_some());
    owned_str(b.slot_mut(2), &key);
    match found.as_ref().and_then(|d| d.get("value")) {
        Some(v) => bson_to_code(b.slot_mut(3), v),
        None => null(b.slot_mut(3)),
    }
    object(out, &[c"_class", c"found", c"key", c"value"], &mut b);
    b.release_all();
    Ok(())
}

fn delete(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let key = require_str(particle, "key", "Delete")?;
    let n = kv_collection(particle)?
        .delete_one(doc! { "_id": key })
        .run()
        .map_err(|e| format!("Delete failed: {e}"))?
        .deleted_count;
    one_bool(out, c"DeleteResult", c"existed", n > 0);
    Ok(())
}

fn kv_collection(particle: &CodeValue) -> Result<Collection<Document>, String> {
    let name = find_field(particle, "collection")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(KV_COLLECTION);
    Ok(database()?.collection(name))
}

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

fn insert(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let doc = require_doc(particle, "doc", "Insert")?;
    let id = collection(particle)?
        .insert_one(doc)
        .run()
        .map_err(|e| format!("Insert failed: {e}"))?
        .inserted_id;
    one_str(out, c"InsertResult", c"id", &id_string(&id));
    Ok(())
}

fn insert_many(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let arr = find_field(particle, "docs")
        .filter(|v| v.tag == CodeTag::Array)
        .ok_or("InsertMany requires an array 'docs'")?;
    let docs: Vec<Document> = array_elems(arr)
        .map(|v| bson_doc(v).ok_or("every element of 'docs' must be an object"))
        .collect::<Result<_, _>>()?;
    if docs.is_empty() {
        one_number(out, c"InsertManyResult", c"count", 0.0);
        return Ok(());
    }
    let n = collection(particle)?
        .insert_many(docs)
        .run()
        .map_err(|e| format!("InsertMany failed: {e}"))?
        .inserted_ids
        .len();
    one_number(out, c"InsertManyResult", c"count", n as f64);
    Ok(())
}

fn find(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let coll = collection(particle)?;
    let filter = optional_doc(particle, "filter")?;
    let mut query = coll.find(filter);
    if let Some(sort) = optional_doc_opt(particle, "sort")? {
        query = query.sort(sort);
    }
    if let Some(n) = whole_number(particle, "limit")? {
        query = query.limit(n);
    }
    if let Some(n) = whole_number(particle, "skip")? {
        query = query.skip(n as u64);
    }

    let docs: Vec<Document> = query
        .run()
        .map_err(|e| format!("Find failed: {e}"))?
        .collect::<Result<_, _>>()
        .map_err(|e| format!("Find failed while reading: {e}"))?;

    let mut items = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(docs.len());
    for (i, d) in docs.iter().enumerate() {
        bson_to_code(buf.slot_mut(i as i64), &Bson::Document(d.clone()));
    }
    array(&mut items, &mut buf);
    buf.release_all();

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"FindResult");
    copy(b.slot_mut(1), &items);
    number(b.slot_mut(2), docs.len() as f64);
    object(out, &[c"_class", c"items", c"count"], &mut b);
    b.release_all();
    release(&mut items);
    Ok(())
}

fn count(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let n = collection(particle)?
        .count_documents(optional_doc(particle, "filter")?)
        .run()
        .map_err(|e| format!("Count failed: {e}"))?;
    one_number(out, c"CountResult", c"count", n as f64);
    Ok(())
}

fn collection(particle: &CodeValue) -> Result<Collection<Document>, String> {
    let name = require_str(particle, "collection", "this handler")?;
    Ok(database()?.collection(name))
}

/// `Drop { collection }` → `DropResult { dropped }` — remove the whole
/// collection. `dropped = false` when it didn't exist.
fn drop_collection(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let name = require_str(particle, "collection", "Drop")?;
    let db = database()?;
    let existed = db
        .list_collection_names()
        .run()
        .map_err(|e| format!("Drop failed: {e}"))?
        .iter()
        .any(|c| c == name);
    if existed {
        db.collection::<Document>(name)
            .drop()
            .run()
            .map_err(|e| format!("Drop failed: {e}"))?;
    }
    one_bool(out, c"DropResult", c"dropped", existed);
    Ok(())
}

// ---------------------------------------------------------------------------
// BSON <-> CodeValue
// ---------------------------------------------------------------------------

/// A code value to BSON. `_class` is dropped (it's the language's own
/// injected field), the same rule the `json` module follows.
fn code_to_bson(v: &CodeValue) -> Bson {
    match v.tag {
        CodeTag::Number => {
            let n = v.number;
            if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
                Bson::Int64(n as i64)
            } else {
                Bson::Double(n)
            }
        }
        CodeTag::Str => Bson::String(read_str(v).unwrap_or_default().to_owned()),
        CodeTag::Bool => Bson::Boolean(read_bool(v).unwrap_or(false)),
        CodeTag::Null => Bson::Null,
        CodeTag::Array => Bson::Array(array_elems(v).map(code_to_bson).collect()),
        CodeTag::Object => {
            let mut d = Document::new();
            for (key, value) in object_entries(v) {
                if key != "_class" {
                    d.insert(key, code_to_bson(value));
                }
            }
            Bson::Document(d)
        }
    }
}

fn bson_doc(v: &CodeValue) -> Option<Document> {
    match code_to_bson(v) {
        Bson::Document(d) => Some(d),
        _ => None,
    }
}

/// BSON into `out` as a code value. `ObjectId`/`DateTime` become strings;
/// numbers all land as the language's one Number type; anything exotic
/// (`Binary`, `Decimal128`, …) is its `Display` string rather than being
/// dropped.
fn bson_to_code(out: &mut CodeValue, b: &Bson) {
    match b {
        Bson::Double(n) => number(out, *n),
        Bson::Int32(n) => number(out, *n as f64),
        Bson::Int64(n) => number(out, *n as f64),
        Bson::String(s) => owned_str(out, s),
        Bson::Boolean(v) => boolean(out, *v),
        Bson::Null | Bson::Undefined => null(out),
        Bson::ObjectId(oid) => owned_str(out, &oid.to_hex()),
        Bson::DateTime(dt) => owned_str(out, &dt.try_to_rfc3339_string().unwrap_or_else(|_| dt.to_string())),
        Bson::Array(a) => {
            let mut buf = SlotBuffer::new(a.len());
            for (i, item) in a.iter().enumerate() {
                bson_to_code(buf.slot_mut(i as i64), item);
            }
            array(out, &mut buf);
            buf.release_all();
        }
        Bson::Document(d) => {
            let keys: Vec<&str> = d.keys().map(String::as_str).collect();
            let mut buf = SlotBuffer::new(d.len());
            for (i, value) in d.values().enumerate() {
                bson_to_code(buf.slot_mut(i as i64), value);
            }
            object_dyn(out, &keys, &mut buf);
            buf.release_all();
        }
        other => owned_str(out, &other.to_string()),
    }
}

fn id_string(id: &Bson) -> String {
    match id {
        Bson::ObjectId(oid) => oid.to_hex(),
        Bson::String(s) => s.clone(),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

fn database() -> Result<Database, String> {
    DB.lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or_else(|| NOT_CONNECTED.to_string())
}

fn require_str<'a>(particle: &'a CodeValue, name: &str, class: &str) -> Result<&'a str, String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{class} requires a non-empty string '{name}'"))
}

fn require_doc(particle: &CodeValue, name: &str, class: &str) -> Result<Document, String> {
    find_field(particle, name)
        .and_then(bson_doc)
        .ok_or_else(|| format!("{class} requires an object '{name}'"))
}

fn optional_doc(particle: &CodeValue, name: &str) -> Result<Document, String> {
    Ok(optional_doc_opt(particle, name)?.unwrap_or_default())
}

fn optional_doc_opt(particle: &CodeValue, name: &str) -> Result<Option<Document>, String> {
    match find_field(particle, name) {
        None => Ok(None),
        Some(v) if v.tag == CodeTag::Object => Ok(Some(bson_doc(v).unwrap_or_default())),
        Some(_) => Err(format!("'{name}' must be an object")),
    }
}

fn whole_number(particle: &CodeValue, name: &str) -> Result<Option<i64>, String> {
    match find_field(particle, name) {
        None => Ok(None),
        Some(v) => {
            let n = read_number(v).ok_or_else(|| format!("'{name}' must be a number"))?;
            if n.fract() != 0.0 || n < 0.0 {
                return Err(format!("'{name}' must be a whole number, 0 or greater"));
            }
            Ok(Some(n as i64))
        }
    }
}

/// Hide a password in a `mongodb://user:pass@host` URL before it lands in an
/// `Exception` message.
fn redact(url: &str) -> String {
    match (url.find("://"), url.find('@')) {
        (Some(s), Some(at)) if at > s + 3 => format!("{}****@{}", &url[..s + 3], &url[at + 1..]),
        _ => url.to_string(),
    }
}

fn ok_result(out: &mut CodeValue, class: &'static std::ffi::CStr) {
    one_bool(out, class, c"ok", true);
}

fn one_str(out: &mut CodeValue, class: &'static std::ffi::CStr, key: &'static std::ffi::CStr, value: &str) {
    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), class);
    owned_str(b.slot_mut(1), value);
    object(out, &[c"_class", key], &mut b);
    b.release_all();
}

fn one_bool(out: &mut CodeValue, class: &'static std::ffi::CStr, key: &'static std::ffi::CStr, value: bool) {
    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), class);
    boolean(b.slot_mut(1), value);
    object(out, &[c"_class", key], &mut b);
    b.release_all();
}

fn one_number(out: &mut CodeValue, class: &'static std::ffi::CStr, key: &'static std::ffi::CStr, value: f64) {
    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), class);
    number(b.slot_mut(1), value);
    object(out, &[c"_class", key], &mut b);
    b.release_all();
}


# `mongodb` — a MongoDB collection

Two layers over one connection: general document CRUD on a named collection,
and a key/value shortcut for the common "one blob per key" case.

```code
link "mongodb.so" as db

emit Config { url = "${MONGO_URI}", database = "myapp" } to db get c
assert c.ok

emit Insert { collection = "events", doc = { kind = "signup", user = "u1" } } to db get _
emit Find   { collection = "events", filter = { kind = "signup" }, sort = { _id = -1 }, limit = 20 } to db get r
-- r.items is an array of objects
```

## Handlers

```
Config     { url, database }                              → ConfigResult      { ok }
Store      { key, value, collection? }                    → StoreResult       { key }
Fetch      { key, collection? }                           → FetchResult       { found, key, value }
Delete     { key, collection? }                           → DeleteResult      { existed }
Insert     { collection, doc }                            → InsertResult      { id }
InsertMany { collection, docs }                           → InsertManyResult  { count }
Find       { collection, filter?, sort?, limit?, skip? }  → FindResult        { items, count }
Count      { collection, filter? }                        → CountResult       { count }
Drop       { collection }                                 → DropResult        { dropped }
```

`Config` is the setup particle — everything else is an `Exception` until it
has run. Connection timeouts come from the URL
(`?serverSelectionTimeoutMS=…`) or the driver's defaults; this module does
not clamp them.

## Key/value

`Store { key, value }` upserts `{ _id: key, value: <value> }` into a `state`
collection (pass `collection` to use another). `Fetch` reads it back;
`Delete` removes it. The value keeps its shape — an object comes back an
object, a number a number — the same round-trip guarantee `json_store` and
the `json` module give.

Use `json_store` for a handful of small files; use this when the data is
already in Mongo or there's too much of it for the filesystem.

## Documents

`Insert` / `InsertMany` / `Find` / `Count` are the driver's operations with
`filter`, `sort`, `limit`, `skip` passed as plain objects and numbers.
`Find` returns **`items`, an array of objects** — the euglena organelle
returned a JSON string because the old ABI had no way to build an array; it
does now.

`InsertResult.id` is the new document's `_id` as a string (an `ObjectId` in
hex, or the string you supplied).

## BSON ↔ values

| BSON | value |
|---|---|
| `Double` / `Int32` / `Int64` | Number (the language has one) |
| `String` / `Boolean` / `Null` | String / Boolean / null |
| `ObjectId` | its hex string |
| `DateTime` | an RFC 3339 string |
| `Array` / `Document` | Array / Object |
| anything else (`Binary`, `Decimal128`, …) | its textual form, rather than dropped |

Going the other way, a whole-numbered Number is stored as `Int64`. `_class`
— the language's own injected field — is dropped from every document, the
same as in the `json` module.

## The decisions, and why

**Ported from `euglena-language`'s `mongodb` organelle**, with these changes:

- **`Find` returns an array**, not `items` as a JSON string.
- **`Sap` became `Config`**, and a failed operation is an `Exception`, where
  the organelle returned `{ ok: false }` on every result.
- **`Drop` is new** — a test that can't reset its collection isn't
  reproducible.
- **Timeouts are the URL's**, not a hard-coded 30s, so a fixture can ask for
  a fast failure.

**No aggregation pipeline, no transactions, no change streams, no indexes.**
Each is a real feature to add when asked; the euglena apps needed
insert/find/count and a KV layer.

**The connection is per link.** `Config` replaces it. An app that talks to
two databases links `mongodb` twice.

## Build

```sh
cargo build --release        # -> target/release/libmongodb.so
```

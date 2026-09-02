# `mongodb_mock` — `mongodb` without a server

A drop-in for [`mongodb`](../mongodb/README.md): the same nine particles and
the same result shapes, over in-memory collections that live for the
process.

```code
link "mongodb_mock.so" as db

emit Config { url = "mock", database = "app" } to db get c

emit Store { key = "settings", value = { theme = "dark" } } to db get _
emit Fetch { key = "settings" } to db get f
assert f.value = { theme = "dark" }

emit Insert { collection = "events", doc = { kind = "signup" } } to db get _
emit Find { collection = "events", filter = { kind = "signup" } } to db get r
| r.items is an array of objects
```

## What `Find` supports

The subset the euglena apps use: **exact-match** on any field in `filter`,
`sort` by one key (`{ field = 1 }` / `{ field = -1 }`), `limit`, `skip`. No
operators (`$gt`, `$in`, …), no aggregation pipeline — the same feature set
the real module ships, minus the server.

Values keep their shape and field order on the round trip, and a document
without `_id` gets one (`mock<hex>`), matching `mongodb`.

## Build

```sh
cargo build --release        # -> target/release/libmongodb_mock.so
```

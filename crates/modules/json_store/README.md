# `json_store` — a file-backed key-value store

One JSON file per key, under a base directory `Config` establishes. The files are
readable and hand-editable on purpose — this is for lightweight runtime
state (which apps a user stopped, a feature flag, a small cached document),
not a database.

```code
link "json_store.so" as store

emit Config { base_dir = "/var/lib/myapp/state" } to store get _

emit Store { key = "prefs", value = { theme = "dark", size = 14 } } to store get _
emit Fetch { key = "prefs" } to store get f
assert f.value = { theme = "dark", size = 14 }
```

## Handlers

```
Config { base_dir }        → ConfigResult { ok, base_dir }
Store  { key, value }       → StoreResult  { key }
Fetch  { key }              → FetchResult  { exists, key, value }
Delete { key }              → DeleteResult { key, existed }
Remove { key }              → DeleteResult { key, existed }   -- an alias for Delete
```

`Config` is the setup particle — a stateful module. `Store`/`Fetch`/`Delete`
are an `Exception` until it has run.

- `Store` takes **any** value and writes `<base>/<key>.json`. `Store { key }`
  with no `value` writes `null` — distinct from `Delete`, which removes the
  file. The write is atomic (temp file + rename).
- `Fetch` on an absent key answers `{ exists = false, value = null }`, not an
  `Exception`.
- `Delete` / `Remove` are the same handler and idempotent: `existed = false`
  when the key wasn't there.

## Values keep their shape

A stored object comes back an object, a number a number, in the same field
order — `Store` then `Fetch` is the identity for any value with no `_class`
field (which `Store` drops, like the `json` module). The on-disk file is the
value's own JSON, pretty-printed, nothing wrapped around it.

## Keys are filenames

A key must be a non-empty run of `[A-Za-z0-9._:@-]` and not be `.` or `..`.
Anything else is an `Exception`. The euglena organelle this was ported from
rewrote every other character to `_`, which silently mapped `a/b` and `a_b`
onto one file; refusing is safer than a collision the caller can't see.

## The decisions, and why

**Ported from `euglena-language`'s `json_store` organelle**, with four
changes:

- **`Sap` became `Config`.** `Sap` is euglena's manifest-delivery
  mechanism; a `code` module's setup is a particle the program sends.
- **The value is stored directly.** The organelle wrote
  `{"key": …, "value": "<the value, as a JSON string>"}` — a string inside an
  object — and on `Fetch` pulled the string out and tried to re-parse it.
  This writes the value's own JSON.
- **Bad keys are refused**, not character-substituted (see above).
- **Failures are `Exception`s**, not `Error { status_code }`. A corrupt file
  on `Fetch` is an `Exception`; an absent key is a normal `FetchResult`.

**No listing, no transactions, no TTL.** A store that needs any of those has
outgrown "a file per key" and wants `mongodb` or similar.

## Build

```sh
cargo build --release        # -> target/release/libjson_store.so
```

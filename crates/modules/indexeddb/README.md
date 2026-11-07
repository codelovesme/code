# `indexeddb` — what a page keeps that is too large for `storage`

`storage` is the browser's `localStorage`: text, a few megabytes shared by
every application on the origin, answered at once. This is IndexedDB: any
value (objects, lists, long base64 text — a photo, a recording), as much as
the browser lets the origin keep, until the application or the person
removes it.

```code
link "indexeddb.a" as kept

emit Put { key = "keepsake:ada:media:m1", value = { type = "image/jpeg", data = photo } } to kept get asked
| asked.ok, asked.id — the answer is later:

DbStored { id, key } => …            | kept: safe through a reload
DbFailed { id, key, reason, full } => …   | not kept — `full` when out of space
```

## The handlers

Each answers `<Name>Result { ok, id }` at once; the answer arrives later,
carrying the same `id`.

```
Put { key, value }  → later DbStored  { id, key }
Get { key }         → later DbFound   { id, key, value }       | null: nothing kept
Delete { key }      → later DbDeleted { id, key }
Keys { prefix? }    → later DbKeys    { id, prefix, keys }      | sorted
Space {}            → later DbSpace   { id, usage, quota, persisted }
Persist {}          → later DbSpace   { … }   after asking the browser to keep
                                              this origin through storage pressure
```

Any failure is `DbFailed { id, key, reason, full }`. `DbStored` is sent only
once the write has committed: a page may say "kept" when, and only when, it
has it.

**One store per origin**, like `storage`: an application keeps its keys
under its own prefix. **Later, because IndexedDB is asynchronous**: waiting
for it would block the page, so the answer is a particle of its own, the
way `net_client`'s replies and `media`'s recordings arrive.

## Where it works

A browser. On a machine every handler answers an `Exception` — a program
with no page keeps things with `fs` or a database.

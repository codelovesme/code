# `objects` — an object used as a map

Reading a key by name (`m[k]`), setting one (`m + { "$k" = v }`) and walking
them (`loop k, v over m`) are the language's own. This module answers what it
could not: whether a key is there at all when its value is null, how to drop
one, and an object's keys or values as lists.

```code
link "objects" as objects

m = { a = 1, b = null }
emit Has { value = m, key = "b" } to objects get has
assert has.value                      | there, though its value is null
emit Delete { value = m, key = "a" } to objects get rest
assert rest.value = { b = null }
emit Keys { value = m } to objects get keys
assert keys.value = ["a", "b"]
```

| Handler | Fields | Answers |
|---|---|---|
| `Keys` | `value` (object) | `KeysResult { value }`: its keys, in order |
| `Values` | `value` (object) | `ValuesResult { value }`: its values, in order |
| `Has` | `value` (object), `key` (text) | `HasResult { value }`: whether the key is there |
| `Delete` | `value` (object), `key` (text) | `DeleteResult { value }`: the object without that key (unchanged if absent) |

A `value` that is not an object, or a `key` that is not text, answers an
`Exception` with source `objects`. Stateless: every link shares one instance.

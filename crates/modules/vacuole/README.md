# `vacuole` — memory that stays within a budget

A key→value store held in the memory of the program that links it: what a
long-running server keeps close because fetching it again costs time, never
what it cannot afford to lose. The native counterpart of the browser-only
Vacuole organelle (a WASM module over a JavaScript map), with the same
Get/Set/Remove contract and the bound a server needs.

```code
link "vacuole" as cache

emit Config { budget_bytes = 8388608 } to cache get _
emit Set { key = "topic:home", value = { title = "Home", words = ["heating"] } } to cache get stored
| stored.evicted names what had to go to make room — drop it from any index.
emit Get { key = "topic:home" } to cache get hit
assert hit.found
```

| Handler | Answer |
| --- | --- |
| `Config { budget_bytes }` | `ConfigResult { ok, budget_bytes, evicted }` — default 64 MiB; `0` turns the store off; lowering it evicts at once |
| `Set { key, value }` | `SetResult { ok, bytes, evicted }` — least recently used go first; a value larger than the whole budget is refused (`ok = false`, `reason`) |
| `Get { key }` | `GetResult { found, value }` |
| `GetMany { keys }` | `GetManyResult { items = [{ key, found, value }] }` — at most 1000 keys |
| `Remove { key }` | `RemoveResult { removed }` |
| `RemovePrefix { prefix }` | `RemovePrefixResult { removed }` — a count; the prefix must not be empty |
| `Keys { prefix?, limit? }` | `KeysResult { items }` — sorted; not counted as a use |
| `Stats` | `StatsResult { count, bytes, budget_bytes, evictions, hits, misses }` |

**A value is a copy.** It is kept as its JSON (the `json` module's rules:
`_class` is dropped, whole numbers stay whole), so changing what `Get`
answered changes nothing stored.

**What the budget counts.** The key, the JSON text and 64 bytes per entry.
It bounds what this store holds, not the process's memory (the runtime's
own copies of a value in flight are not counted).

**Eviction is not deletion.** Nothing here was ever the only copy; a caller
fetches an evicted value again from where it came from. Forgetting
something for good means removing it here *and* at its source.

Each `link` has its own store; keys are non-empty text of at most 512 bytes.
Native only.

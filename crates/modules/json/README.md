# `json` — JSON text in and out

The language already renders any value as compact JSON through string
interpolation:

```code
let payload = { ok = true, items = [1, 2] }
assert "$payload" = "{\"ok\":true,\"items\":[1,2]}"
```

So this module is the two things interpolation cannot do: **parse** JSON text
back into a value, and **pretty-print** one.

```code
link "json.so" as json

emit Parse { text = "{\"name\":\"ada\",\"wins\":[1,2,3]}" } to json get p
assert p.value.name = "ada"
assert p.value.wins = [1, 2, 3]

emit Stringify { value = { a = 1 }, pretty = true } to json get s
assert s.value = "{\n  \"a\": 1\n}"
```

## Handlers

```
Parse     { text }            → ParseResult     { value }
Stringify { value, pretty? }  → StringifyResult { value }
```

| Field | Kind | Default | Meaning |
|---|---|---|---|
| `Parse.text` | String | — | the JSON document. A missing or non-string `text`, or text that isn't valid JSON, comes back as an `Exception` |
| `Stringify.value` | any | `null` | the value to serialize — `Stringify { }` serializes `null` |
| `Stringify.pretty` | Bool | `false` | `true` indents nested structure with two spaces; `false` is compact, byte-for-byte what `"$value"` produces |

`ParseResult.value` is the parsed value; `StringifyResult.value` is the JSON
string. Both wrap their payload the way every handler in this repo does —
`emit … get r`, then `r.value`.

## `_class` is dropped, nothing else is

Every particle and handler result carries a `_class` field the language
injects. `Stringify` drops it — from the top-level object and from every
nested one — so `Stringify { value = received }` gives the data a program is
carrying, not the plumbing:

```code
Handle { id } => {
    -- `received` here is `{ _class = "Handle", id = … }`
    emit Stringify { value = received } to json get s
    -- s.value is `{"id":…}`, no _class
}
```

No other `_`-prefixed key is touched. A `_id` from a database row survives a
round trip.

## Numbers

The value model is JSON's, so there is one number type (`f64`).

- A whole number writes without a fractional part — `3`, not `3.0` — the
  same rule interpolation follows.
- `Parse` maps both `1` and `1.0` to the same value; there is no separate
  integer type to preserve.
- No bignum: a value outside `f64`'s exact-integer range loses precision,
  exactly as it would anywhere else in the language.
- No `NaN`/`Infinity`: JSON cannot spell them, so a non-finite number
  serializes as `null` rather than failing the whole document. `Parse` never
  produces one — the JSON grammar has no syntax for it.

## Field order

`Stringify` preserves the order a program's fields were written
(`{ z = 1, a = 2 }` → `{"z":1,"a":2}`), and `Parse` preserves a document's
order. This is what keeps `Stringify` in step with interpolation, whose
output is also in declaration order — and it matters here because the
language compares objects field-by-field *in order*, so a reordered round
trip would not be equal to the original.

## The decisions, and why

**Ported from `euglena-language`'s `json` organelle**, with two changes:

- **Failure is an `Exception`, not an `Error`.** The organelle returned
  `Error { status_code, message }` (and `null` for a parse failure); a module
  in this language reports by returning `Exception { source, message }`,
  which the program receives as an ordinary value and may test with `∈
  Exception` or ignore. A module may never end the program, and a silent
  `null` for bad input is its own trap.
- **Only `_class` is dropped**, where the organelle dropped every
  `_`-prefixed key. Dropping `_id` made the organelle unable to round-trip a
  database row, which is most of what a JSON module is for.

**No streaming, no partial parse, no JSON5.** `Parse` takes a whole document
and returns a whole value. A document too large to hold in memory is a
different problem than this module solves.

**`Stringify` compact output is not just "close to" `"$value"` — it is the
same bytes.** If that ever stops being true it is a bug in one of the two.

## Build

```sh
cargo build --release        # -> target/release/libjson.so
```

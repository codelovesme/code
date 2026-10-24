# `csv` — rows of text in and out

Read a spreadsheet export, a bank statement or a price list; write one back.
Quoting follows RFC 4180: a field with the separator, a quote or a line break
in it is quoted, and a quote inside is doubled.

```code
link "csv.so" as csv

emit Parse { value = "name,price\nWidget,007\n" } to csv get sheet
assert sheet.value = [{ name = "Widget", price = "007" }]
emit Stringify { value = sheet.value } to csv get text   | "name,price\nWidget,007\n"
```

## Handlers

```
Parse     { value, header?, separator? }  → ParseResult     { value }   | the rows
Stringify { value, header?, separator? }  → StringifyResult { value }   | the text
```

**Stateless** — there is nothing to configure and no setup particle.

| Field | Kind | Meaning |
|---|---|---|
| `header` | Bool, default true | Parse: the first line names the fields, and each row is an object by those names; false: each row is an array. Stringify: write the header line |
| `separator` | String, default `","` | one character, not a quote or a line break (`";"`, `"\t"`) |
| `Parse.value` | String | the CSV text |
| `Stringify.value` | Array | rows, all objects or all arrays |

- **Every parsed field is text.** `007` is a code and `12.50` may be a price
  to keep as written; only the program knows which, so it converts
  (`math`'s `Parse`) where it means a number.
- **Stringify** writes rows of objects under a header naming every key, in
  the order the keys first appear; a row without a key leaves its field
  empty. A number is written as the language writes it, a bool as
  `true`/`false`, null as an empty field. Rows of arrays are written as they
  are, and may differ in length.
- A row with more or fewer fields than the first, text that is not UTF-8, a
  list or an object as a field, or a bad option answers an `Exception`
  saying which (`line 2 has 3 fields, the first has 2`).

Native only: there is no browser half.

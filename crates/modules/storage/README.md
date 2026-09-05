# `storage` — what the browser remembers

```code
link "storage.a" as store

emit Set { key = "last_route", value = "/prices" } to store get saved
assert saved.ok

emit Get { key = "last_route" } to store get seen
assert seen.value = "/prices"

emit Remove { key = "last_route" } to store get gone
```

## The handlers

```
Get { key }           → GetResult { value }      | null when nothing is there
Set { key, value }    → SetResult { ok }
Remove { key }        → RemoveResult { ok }
```

`ok` is false when the browser refused — a full store, or a reader who has
turned storage off. It is not an exception: a page that cannot remember is a
page that has to carry on.

`Get` answers **null** for a key that was never set, and `""` for one holding
an empty string. Those are different answers and this module keeps them
apart.

## Text, and only text

What a browser stores is a string, and this module does not pretend
otherwise. An application with an object to keep turns it into text with the
`json` module and stores that.

Two modules, each doing one thing, rather than a store that quietly
serialises — and a stored value that turns out to be unparseable is then the
application's to answer, at the point where it knows what it expected.

## Where it works

**A browser.** On a machine every handler answers an `Exception` saying so.
A machine has a filesystem and an `fs` module for exactly this, which is why
this one does not quietly fall back to it: they keep different things. What
the browser remembers is per reader and per site, and lives as long as that
reader lets it.

The module is still linkable on a machine, so one application can be built
both ways and ask [`Linked`](../../../README.md#linked) which it is.

For wasm it is built as an archive linked into the program:

```bash
cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
```

Its page half is in [`web/host.mjs`](../../../web/host.mjs), with every other
browser module's.

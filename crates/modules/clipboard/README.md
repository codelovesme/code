# `clipboard` — what the reader copies

```code
link "clipboard.a" as clip

emit Copy { text = "the line to keep" } to clip get copied
assert copied.ok

CopyFailed { reason } =>
    | the browser said no after the fact — outside a click, or on an
    | insecure page
```

## The handlers

```
Copy { text }   → CopyResult { ok }   · later, on refusal: CopyFailed { reason }
```

`ok` is false when the browser offers no clipboard at all. A browser that has
one may still refuse the write, and says so only after the answer has gone —
so that refusal arrives as its own particle.

## Its own module

The clipboard is not the page: `dom` draws, `storage` remembers, and this
copies. A program that wants one links one.

## Where it works

**A browser.** On a machine every handler answers an `Exception` saying so.
The module is still linkable there, so one application can be built both
ways and ask [`Linked`](../../../README.md#linked) which it is.

For wasm it is built as an archive linked into the program:

```bash
cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
```

Its page half is in `page.mjs`, with every other browser module's.

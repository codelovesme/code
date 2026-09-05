# `dom` — a page drawn from a value

```code
link "dom.so" as dom

emit Render {
    into = "#app",
    tree = {
        tag = "section",
        attrs = { class = "cart" },
        children = [
            { tag = "h1", children = ["Sepet"] },
            { tag = "p", attrs = { class = "total" }, children = ["Toplam: $total TL"] }
        ]
    }
} to dom get r
assert r.ok
```

## Handlers

```
Render { into?, tree } → RenderResult { ok }
Style  { css }         → StyleResult  { ok }
```

`into` is a CSS selector, `"body"` by default; `ok` is false when it matched
nothing. `Style` replaces the sheet it put there last time rather than
stacking a new one, so an application can restyle itself.

## The tree is a value, not markup

A node is `{ tag, attrs?, children? }` and a string is a text node. That is
the whole vocabulary: **no raw HTML, no event handlers, no property
assignment.** A tree built out of someone's name or a translated string is
data all the way to the page and cannot become code on the way — the page's
half is held to the same rule, and `<`/`>` are escaped even inside JSON
strings so a serialised tree can never close a tag.

`tree` may also be a **string**, taken as JSON already in this shape and
passed through untouched — for an application that built the text itself.

## Appearance does not belong in the tree

A node says what it *is*:

```code
{ tag = "p", attrs = { class = "total" }, children = ["Toplam: $total TL"] }
```

No colour, no position, no spacing. What `total` looks like belongs in a
stylesheet — a file the page loads, or one the application sends through
`Style` if it would rather ship a single file. Keeping appearance out is
what stops the code that builds a page from becoming the page's design.

## What the page has to supply

Two imported functions, and they are the only things this module can reach:
one that renders a tree into a selector, one that sets the stylesheet. Both
take text and answer whether they matched. A page that supplies neither gets
a link error naming them, rather than a module that silently draws nowhere.

The page's half is small — parse the JSON, create elements, set attributes,
append children — and it must refuse anything else. An `on*` attribute, an
`innerHTML`, a property set by name: none of those can be reachable, or the
guarantee above is gone.

## Where it works

**A browser.** On a machine every handler answers an `Exception` saying so.
The module is still linkable there, so one application can be built both ways
and ask [`Linked`](../../../README.md#linked) which it is.

For wasm it is built as an archive linked into the program:

```bash
cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
```

## Why the wasm half brings no standard library

**Two `code-native` modules cannot be linked into one `.wasm` at all.** Each
carries its own copy of Rust's standard library, so its private symbols end
up defined twice, and the wasm linker has no flag to forgive that the way a
native one does. A web application links several modules by definition, so
the module that is *for* the web is the one that must bring nothing: its wasm
half is `no_std` and hand-written against `code_abi.h`.

It is also most of the size — about 25 KB against 245 KB, per module.

The lasting fix is a `no_std` mode for `code-native`, at which point this
module collapses back into one implementation. Until then: **a module meant
for wasm brings no standard library.**

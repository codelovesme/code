# `dom` — a page drawn from a value

```code
link "dom.so" as dom

emit Render {
    into = "#app",
    styles = {
        ".cart"  = { "max-width" = "24rem", padding = "1rem 1.4rem" },
        ".total" = { "font-weight" = "600" }
    },
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

## The handler

```
Render { into?, styles?, tree } → RenderResult { ok }
```

`into` is a CSS selector, `"body"` by default; `ok` is false when it matched
nothing. `styles` replaces the sheet set last time rather than stacking a new
one, so an application can restyle itself.

One handler, one payload. The rules and the tree travel together, because
they describe one page.

## The tree is a value, not markup

A node is `{ tag, attrs?, children? }` and a string is a text node. That is
the whole vocabulary: **no raw HTML, no event handlers, no property
assignment.** A tree built out of someone's name or a translated string is
data all the way to the page and cannot become code on the way — the page's
half is held to the same rule, and `<`/`>` are escaped even inside JSON
strings so a serialised tree can never close a tag.

`tree` may also be a **string**, taken as JSON already in this shape and
passed through untouched — for an application that built the text itself.

## A click is a particle

A node may say what an event on it *means*:

```code
{ tag = "input",  on = { input = "Typed" } }
{ tag = "button", attrs = { value = "7" }, on = { click = "Remove" } }
```

`on` maps an event name to a **class**. When it happens, the page fires that
class together with the element's own value and the runtime builds the
particle — `Typed { value = "ne yazdıysa" }`, `Remove { value = "7" }` — and
the program answers it with an ordinary handler:

```code
Typed { value } => {
    draft = value
    return Noted {}
}
```

The element's value is what lets one shape serve every component. A button
carries what the program wrote on it, a text box what the reader typed, a list
what was chosen; all of them arrive the same way, so a handler for one reads
like a handler for any other. An event with nothing to carry — a click on a
plain button — makes a particle with no `value` field at all.

**A listener is never a function, and nothing is held between renders.** `on`
is data like every other field: this module serialises it and forgets it.
There is no table of live listeners to grow, go stale or be swept, and a page
redrawn a thousand times costs exactly what one render costs.

## Appearance travels with it, but not on the nodes

A node says what it *is*:

```code
{ tag = "p", attrs = { class = "total" }, children = ["Toplam: $total TL"] }
```

No colour, no position, no spacing. Those are in `styles`, once, keyed by
selector — **in the same particle**, so there is no stylesheet file to keep
in step with the application and nothing to serve beside it.

That split is the point. Written onto every node, appearance would make the
code that builds a page *be* the page's design, which is what a gene must not
turn into. A genuinely per-node value — a bar's width computed from data — is
an ordinary attribute (`attrs = { style = "width: 40%" }`) and needs nothing
from this module.

`styles` is a value, not CSS text: selector to properties to values. So there
is no stylesheet to parse, and nothing that could end a rule early and start
a different one. The page drops `{`, `}`, `<` and `>` from every name and
value on top of that.

## What the page has to supply

One imported function, and it is the only thing this module can reach: it
takes the payload and the selector, and answers whether the selector matched.
A page that supplies nothing gets a link error naming it, rather than a
module that silently draws nowhere.

The page's half is small — parse the JSON, create elements, set attributes,
append children — and it must refuse anything else. An `on*` attribute, an
`innerHTML`, a property set by name: none of those can be reachable, or the
guarantee above is gone.

Going the other way it calls `code_event_fire`, having written the class and
the value into the runtime's own buffers (`code_event_class`,
`code_event_text`, and the two capacities). The buffers are the runtime's on
purpose: a page choosing where to write in the program's memory could write
anywhere.

## Where it works

**A browser.** On a machine every handler answers an `Exception` saying so.
The module is still linkable there, so one application can be built both ways
and ask [`Linked`](../../../README.md#linked) which it is.

For wasm it is built as an archive linked into the program:

```bash
cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
```

## Why the wasm half brings no standard library

**It is most of the size** — about 25 KB against 245 KB, per module. A web
application links several modules by definition, so the module that is *for*
the web is the one that must bring nothing: its wasm half is `no_std` and
hand-written against `code_abi.h`.

For a while it was worse than a size question. Every archive Rust produces
carries a panic handler and an unwinding personality, so two Rust modules in
one program define them twice and the link failed outright. Nothing can be
done about that from inside a module — a `staticlib` must carry a panic
handler, and the program that links them is not Rust and has none to offer —
so `code build` allows the duplicate and checks the case that actually
matters, two modules sharing an export prefix, by name instead.

The lasting fix for the size is a `no_std` mode for `code-native`, at which
point this module collapses back into one implementation. Until then: **a
module meant for wasm brings no standard library.**

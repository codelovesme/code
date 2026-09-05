# `router` — where in the application the reader is

```code
link "router.a" as router

emit Watch { then = "Went" } to router get w
emit Route { } to router get here
emit Draw { path = here.value } to this get _

Went { path } => {
    | every change of path arrives here: a link, Back, Forward, a typed address
}
```

## The handlers

```
Route { }             → RouteResult { value }     | the path shown now
Navigate { path }     → NavigateResult { ok }     | go there
Watch { then }        → WatchResult { ok }        | and tell me when it changes
```

`Watch` names the class the application wants back. From then on every change
of the path arrives as a particle of that class, carrying the new path as
`value`.

`Navigate` fires it too. An application that draws in one place — the handler
— does not then have to draw again at every call site, and the two ways a
path can change stop being two paths through the code.

## Naming the class in advance

The application says what an event should become *before* it happens — the
same rule `dom` follows for a click. What it buys is a fixed **shape**: what
arrives is a class and at most one piece of text, so a page cannot invent a
particle with fields of its own choosing, and a handler can be written against
that.

It is not a boundary. A page and the module it loaded share one memory, and
nothing stops a page from naming a class the application never offered it —
anything that could is already able to write to that memory directly. What
protects an application from the page it runs in is on the other side of the
network, where the two really are separate.

## Where it works

**A browser.** On a machine every handler answers an `Exception` rather than
inventing a path — a program that thought it knew where it was would draw the
wrong page and never find out why. It is still linkable there, so one
application can be built both ways and ask
[`Linked`](../../../README.md#linked) which it is.

For wasm it is built as an archive linked into the program:

```bash
cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
```

Its page half is in [`web/host.mjs`](../../../web/host.mjs), with every other
browser module's. It routes on the URL hash, because a page served as a file
has nothing else it can change without asking a server for a URL that does
not exist.

# `router` — where in the application the reader is, and where the page is

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
Where { }             → WhereResult { origin, protocol, hostname, port }
```

`Watch` names the class the application wants back. From then on every change
of the path arrives as a particle of that class, carrying the new path as
`path`.

`Navigate` fires it too. An application that draws in one place — the handler
— does not then have to draw again at every call site, and the two ways a
path can change stop being two paths through the code.

## `Where` answers what `Route` cannot

`Route` and `Navigate` are the hash — the part of the address an application
controls, hosted or alone. `Where` is everything before it: the scheme, the
host, the port. An application built to talk to a service of its own — its
API, on the same deployment — needs exactly this and nothing else, because no
module knows a deployment's address for it; the page it was loaded from
already *is* that address.

```code
emit Where { } to router get here
let api = here.origin + "/api"
```

`origin` is null for a page with no address at all — one opened straight off
disk (`file://`). Not an exception: this answers what the page *is*, and a
page with no address is a fact about it, not a failure.

**A guest's own `router` never narrows it.** `Route` is scoped there — a
guest's own address is the path after its name, because two applications
cannot both own the address bar — but `Where` reads the page directly rather
than through that per-guest slice, so a guest left to its own `router`
answers the same origin its shell would. A shell that *takes over* `router`
for a guest (see [`guest`](../guest/README.md)'s `Offer`/`Module`) can
answer whatever it likes for `Where` too, the same as for anything else it
took — that is an ordinary decision the shell made, not something this
module arranges on its own.

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
not exist. `Where` reads the page's real address directly rather than
through that hash, which is what keeps it the same answer for a guest as for
the page hosting it.

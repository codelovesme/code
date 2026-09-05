# `web/host.mjs` — the page's half of the browser modules

A module for the browser is two pieces of code. One is compiled into the
`.wasm`: it takes a particle, works out what was asked, and calls a function
it deliberately left undefined. The other is that function, written in the
only language that can reach a page. **Neither is a module on its own.**

This file is the second half of all of them at once:

| Module | What the page supplies |
|---|---|
| `console` | `code_web_log` |
| `dom` | `code_web_render` |
| `storage` | `code_web_storage_get` / `_set` / `_remove` |
| `router` | `code_web_route_get` / `_set` / `_watch` |
| `timer` | `code_web_timer_set` / `_clear` |

plus the four the language itself needs from a host with no operating system
(the clock, the error sink, and turning a double into text and back — a
freestanding build cannot compute those and asks).

A module the application did not link costs nothing but an unused import.

```html
<div id="app"></div>
<script type="module">
  import { runWasm } from "./host.mjs";
  await runWasm("app.wasm");
</script>
```

`createHost({ doc, log })` is the longer form, for a page that wants to keep
the instance — or for a test that hands over a document of its own and checks
what the application drew, with no browser involved.

## Two rules run through all of it

**Nothing here interprets.** No `innerHTML`, no `eval`, no property reached by
name, no handler built from text. A tree built out of someone's name is data
all the way to the page and cannot become code on the way. The `dom` module's
side is held to the same rule.

**Nothing trusts an address or a length that came from outside.** When
something has to come back — a stored value, the current path, the text an
event carries — the module hands over a buffer of its own and says how much
room it has, so its reads stay inside its own array.

That is containment, not a boundary. This file and the module share one
memory; anything running in the page can read and write all of it. What it
buys is that an honest mistake here stops here, instead of becoming a corrupt
value the application then works with.

## Going the other way

Three of these call *in*: a click, a change of path, a delay that elapsed.
The application says, in advance, what class an event should become — `on = {
click = "Add" }`, `Watch { then = "Went" }`, `Delay { then = "Rang" }` — and
this file fires that class with one piece of text alongside. The runtime
builds the particle from those two and the application's own handler answers
it.

So what arrives always has the same shape — a class and at most one piece of
text — and a handler can be written against that. It is not a boundary:
nothing stops a page from naming a class the application never offered it, and
anything that would is already able to write to the program's memory directly.
The place where an application really is separate from what talks to it is the
network, and that is where its answer to "who is allowed to ask for this"
belongs.

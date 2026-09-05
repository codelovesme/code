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

**The page never chooses an address in the program's memory.** When something
has to come back — a stored value, the current path, the text an event
carries — the module hands over a buffer of its own and says how much room it
has. A page that could name an address could write anywhere.

## Going the other way

Three of these call *in*: a click, a change of path, a delay that elapsed.
None of them invents a particle. The application says, in advance, what class
an event should become — `on = { click = "Add" }`, `Watch { then = "Went" }`,
`Delay { then = "Rang" }` — and the page can only fire what it was given,
with one piece of text alongside. The runtime builds the particle from those
two and the application's own handler answers it.

So the wire coming in is exactly as narrow as the wire going out. A page
cannot reach a handler the application did not offer it.

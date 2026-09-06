# The page's half of the browser modules

A module for the browser is two pieces of code. One is compiled into the
`.wasm`: it takes a particle, works out what was asked, and calls a function
it deliberately left undefined. The other is that function, written in the
only language that can reach a page. **Neither is a module on its own**, so a
module keeps both halves together — its `page.mjs` sits beside its Rust.

`runtime.mjs` here is the part that belongs to no module in particular: the
four functions the language itself needs from a host with no operating system
(a clock, an error sink, and turning a double into text and back — a
freestanding build cannot compute those and asks), plus the wiring that lets
a page fire a particle back.

**`code build --target wasm` writes the two together**, as `host.mjs`, beside
the module it built — `runtime.mjs` with the halves of the modules the
program actually linked pasted in. So:

- an application carries what it linked and not one line more;
- the two halves come out of the same binary in the same second, and cannot
  be a version behind each other;
- there is nothing to install, pin, or keep in step by hand.

```html
<div id="app"></div>
<script type="module">
  import { runWasm } from "./host.mjs";
  await runWasm("app.wasm");
</script>
```

`createHost({ doc, log })` is the longer form, for a page that wants to keep
the instance — or for a test that hands over a document of its own and checks
what the application drew, with no browser involved. `store` and `address`
are the same idea for what is remembered and where the reader is; `guard` is
what puts a program between another one and its modules. All four are what
make a second application on one page possible — see below.

## A half answers particles

**A module speaks particles in both directions** — toward the language and
toward the page. So a half here is a function from the particle its module was
sent to the particle it answers:

```js
(ctx) => ["storage", (particle) => {
  if (particle._class === "Get") return { _class: "GetResult", value: … };
  …
  return null;   // a class this module does not handle
}]
```

Nothing crosses as a pointer, a length, or a shape invented for one module.
There is one import for all of them — `code_web_ask`, a particle in as JSON
and a particle out — so a module's wasm half has nothing to do but hand the
particle over, which is why it is
[written once](../crates/modules/browser_half.rs) and included rather than
typed per module.

**Nothing thrown escapes.** A half that threw would take the program's whole
dispatch down with it, from inside a handler, over something as ordinary as a
browser refusing storage. Everything is caught at the door and becomes an
`Exception` particle — which is what the language reads a failure as anyway,
and what the same module's machine half returns.

The ones with a half here are `console`, `dom`, `storage`, `router`, `timer`,
`net_client` and `guest`.

**A module from outside this repository cannot bring its own half yet**, and
that is the honest limit of this design. The halves are embedded in the
compiler and chosen by the prefix a `.a` exports under. Putting the file
inside the archive was tried: it links, but the wasm linker warns on every
build that the member is neither an object nor bitcode. The way out is a
second published asset beside the archive — release, install and lockfile
work that nothing needs yet.

## Two rules run through all of it

**Nothing interprets.** No `innerHTML`, no `eval`, no property reached by
name, no handler built from text. A tree built out of someone's name is data
all the way to the page and cannot become code on the way. Each module's half
is held to the same rule as its other half.

**Nothing trusts an address or a length that came from outside.** When
something has to come back — a stored value, the current path, a particle
fired back — the module hands over a buffer of its own and says how much room
it has, so its reads stay inside its own array.

That is containment, not a boundary. This file and the module share one
memory; anything running in the page can read and write all of it. What it
buys is that an honest mistake here stops here, instead of becoming a corrupt
value the application then works with.

## Going the other way

Three of these call *in*: a click, a change of path, a delay that elapsed.
The application says, in advance, what the event should *mean* — a whole
particle, written where it is drawn:

```code
{ tag = "button", on = { click = { _class = "Remove", id = 7 } } }
```

The page sends that back as JSON when it happens, adding what it learned in
the meantime: the element's value for a click or a keystroke, the new `path`
for a route, the `value` a delay was given. The runtime reads it into a
particle and the application's own handler answers it.

`on = { click = "Add" }` is the short way of writing `{ _class = "Add" }`, for
an event with nothing else to say.

It is not a boundary. Nothing stops a page from sending a particle the
application never offered it, and anything that would is already able to write
to the program's memory directly. The place where an application really is
separate from what talks to it is the network, and that is where its answer to
"who is allowed to ask for this" belongs.

## Telling, and asking

`host.fire(particle)` tells the program something happened. Nothing comes
back, because a click has happened whether or not the program has an opinion.

`host.ask(particle)` is the other kind: the handler's own answer comes back,
and null when nothing answered. The caller is waiting inside its own call, and
what it does next depends on what it gets.

That is what lets a page put a program in the middle of something. A shell
running another application can be *asked*, in the language, whether the guest
may draw there or store that — rather than deciding it here, in JavaScript,
where it cannot be tested and is not the application's own reasoning.

## One page, two applications

`createHost` is called again by [`guest`](../crates/modules/guest), the module
that runs another application inside this one: a second world on the same
page, with its own `doc`, `store` and `address`, and a `guard` that turns
every module the guest reaches for into a question for the host program —
`Offer` once per module, `Module` per particle. The halves cannot tell the
difference. They are handed a container and a prefix instead of a page and a
key, and they draw and store exactly as they would.

That is the whole of the browser's side of hosting, and it is why these four
are arguments rather than things this file reaches for. `host.stop()` is the
other end of it: a program let go of is one nothing fired at it afterwards
can reach, so a delay it set before it went finds nobody home.

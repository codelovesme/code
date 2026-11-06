# `interpreter` — source text as a linked program

```code
link "interpreter.so" as plasmid

rule = "Changed { kind, house } =>\n    if kind = \"temperature\" and house.temperature.bedroom > 25\n        return Actions { do = [TurnOn { device = \"bedroom-ac\" }] }\n    return Actions { do = [] }\n"

emit Load { source = rule, name = "cool the bedroom" } to plasmid get loaded
emit Send { id = loaded.id, particle = Changed { kind = "temperature", house = house } } to plasmid get answer
| answer ∈ Actions
emit Unload { id = loaded.id } to plasmid
```

## The handlers

```
Load { source, name?, host? }
                           → Loaded { id, version, hash }
                                                  | Exception when the source does not hold up,
                                                  | or (host) asks for a module the base does not offer
Check { source, name?, host? }
                           → Checked { ok, hash, modules, offered, missing, stage?, message? }
Replace { id, source, version?, host? }
                           → Replaced { id, version, hash } | Exception, the old version untouched
Send { id, particle }      → the handler's answer | null for a class it has no handler for;
                                                  | Exception when the handler failed, or no such id
Stop { id } / Start { id } → Stopped / Started { id, was }
Unload { id }              → Unloaded { existed }
Status { id }              → Status { id, name, version, hash, state, busy, modules,
                                      calls, failures, errors = [Failure] }
Loaded {}                  → Programs { items = [{ id, name, version, hash, state,
                                                   modules, failures }] }
```

`Load` puts the text through everything a file gets before it runs — parse,
handler cycles, undefined names — and runs its top level. Any of that
failing is an `Exception` carrying the message, at `Load`: bad source is
refused where it arrives, never stored as a program that will fail later.
The most one `Load` takes is 256 KB.

## Managing what is loaded

Any program linking this module can manage its loaded programs with these
particles alone — and expose them through its own endpoints, under its own
rights. Nothing here is specific to one application.

**Identity.** A program is its `id`: given at `Load`, never reused, the
same across every `Replace`. `name` is what the base calls it (and the
`app` its `Offer`/`Module` questions carry); two programs may share one.
`version` counts up from 1 with each `Replace` that took, and `hash` is
`sha256:` of the source text running now. Saved source — a rule in a
database, a file — is the base's business: `Unload` removes the running
program and nothing else, and deleting saved source removes nothing here.

**States.** `running` answers `Send`s. `stopped` keeps everything — its
top-level values, its failures — and refuses every `Send` (`stage =
"stopped"`) until `Start`. Both are idempotent and say what the program
`was`. `Unload` frees it; a second `Unload` answers `existed = false`, and
any request about it after is refused `absent`. Stopping or unloading one
program touches no other and none of the base's modules. Nothing is
interrupted mid-handler: a stop or unload takes effect at the next `Send`.
A loaded program has no timers or subscriptions of its own — the base owns
every trigger, so a stopped program's triggers are the base's `Send`s, and
they are refused.

**From inside.** A hosted program's handler can reach the base, and the
base can ask about that very program meanwhile. `Status`, `Stop` and
`Unload` work then; a second `Send` into it, or a `Replace` of it, is
refused `busy`. An `Unload` from inside marks it, and it is dropped the
moment its `Send` has answered — never freed while in use.

**Check.** `Check` examines source exactly as `Load` would and runs
nothing: it reports `modules` (every name the source emits to), which of
those the base `offered`, which are `missing`, and — when the text does
not hold up — the `stage` (`source` or `binding`) and `message`. A name
offered is a connection, not a promise the call will succeed.

**Replace.** The new source is built whole — checked, bound, its top level
run — while the old version keeps answering. Only then does it take the
old one's place, under the same `id`, with `version + 1`; the old version's
state is dropped. If any step fails the old version is untouched and the
refusal says why (`source`, `binding`, `activation`). Give `version` to
replace only what you read: a replacer holding an old version is refused
`stale`. Note what activation means: the new top level has *run*, so
anything it emitted has happened even if its top level then failed; this
module cannot undo it. Keep effects in handlers if that matters.

**Failures.** Every `Send` that answered an `Exception`, every call to a
base module that answered one, and every refused `Replace` is a failure of
that program: `Status` lists the newest 16 (`seq`, `version`, `operation`
= `Send` / `Module` / `Replace`, `target` = the class sent or the module
called, `message`, cut to 400 bytes) and counts all of them. A call that
reached the base and failed is a failure, not a success because the name
was offered. Only the reason is kept, never what was sent.

**Refusals** are `Exception`s with a `stage` — `request`, `source`,
`binding` (with `missing`), `activation`, `absent`, `stopped`, `busy`,
`stale` — and the program's `id` when there is one.

## What a loaded program is

A linked module the base made itself, from text, while running. It has its
own handlers and its own top level, sees nothing of the base's, and is
talked to with particles — the standing a `.so` has, without the file.

What it cannot do is reach back. A `link` inside the source is refused at
`Load`: a loaded program links nothing, so the world it knows is what each
particle brings it. That is the point for rules, plugins and anything else
a person may write into a running program: the base decides what goes in
and what to do with what comes out, and a loaded program can neither touch
a device nor call a service nor keep a file.

### Reaching the base's modules: `host = true`

Loaded with `host = true`, a program may `emit … to <name>` and reach the
base's modules by name, without linking anything — the way a gene reaches
its application's organelles:

```code
link "interpreter.so" as plasmid
link "ntfy.so" as push

Offer { app, name } =>
    if name = "push", return Offered
    return Refused

Module { app, name, particle } =>
    if name = "push"
        emit particle to push get answer
        return answer
    return Refused

emit Load { source = "Remind { text } =>\n    emit Notify { message = text } to push get sent\n    return sent\n", name = "reminder", host = true } to plasmid get loaded
```

The base answers the same two questions a hosted guest asks its host
(`code_abi.h` item 10), through `code_abi.h` item 14:

- `Offer { app, name }`, once per name the source emits to, before anything
  runs. Only `Offered` counts; any name not offered refuses the whole
  program at `Load` (`the host does not offer a module to this program:
  …`), and none of its top level has run.
- `Module { app, name, particle }`, for every emit, nested, on the
  dispatching thread; the base's answer is the emit's answer, an
  `Exception` included. `app` is the program's `name` (`loaded` when it has
  none).

So the base decides exactly what its programs may reach and can refuse,
log or reshape any call. A hosted program still cannot `link`, and
without `host` nothing changes: the program reaches nothing.

Two loaded programs are two worlds. One cannot see the other's top level,
and unloading one leaves the rest as they were.

## Where it works

A machine — a `.so` linked by an interpreted or a compiled base alike. The
page has the interpreter already (`crates/code-wasm`), so there is no wasm
build of this module.

Everything runs on the thread that dispatches into the module; a handler
that never returns holds the base, as any linked module's would.

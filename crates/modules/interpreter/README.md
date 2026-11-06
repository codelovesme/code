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
                           → Loaded { id }        | Exception when the source does not hold up,
                                                  | or (host) asks for a module the base does not offer
Send { id, particle }      → the handler's answer | null for a class it has no handler for;
                                                  | Exception when the handler failed, or no such id
Unload { id }              → Unloaded { existed }
Loaded {}                  → Programs { items = [{ id, name, modules }] }
```

`Load` puts the text through everything a file gets before it runs — parse,
handler cycles, undefined names — and runs its top level. Any of that
failing is an `Exception` carrying the message, at `Load`: bad source is
refused where it arrives, never stored as a program that will fail later.
The most one `Load` takes is 256 KB.

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

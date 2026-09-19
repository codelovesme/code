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
Load { source, name? }     → Loaded { id }        | Exception when the source does not hold up
Send { id, particle }      → the handler's answer | null for a class it has no handler for;
                                                  | Exception when the handler failed, or no such id
Unload { id }              → Unloaded { existed }
Loaded {}                  → Programs { items = [{ id, name }] }
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

Two loaded programs are two worlds. One cannot see the other's top level,
and unloading one leaves the rest as they were.

## Where it works

A machine — a `.so` linked by an interpreted or a compiled base alike. The
page has the interpreter already (`crates/code-wasm`), so there is no wasm
build of this module.

Everything runs on the thread that dispatches into the module; a handler
that never returns holds the base, as any linked module's would.

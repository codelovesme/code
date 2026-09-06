# `timer` — a particle, later

```code
link "timer.a" as clock

emit Delay { ms = 5000, then = { _class = "Refresh", what = "prices" } } to clock get d

Refresh { what } => {
    | ...and re-arm here, if it should keep going
}
```

## The handlers

```
Delay { ms, then }  → DelayResult { value }   | the number it is known by
Cancel { id }       → CancelResult { ok }
```

`then` is the particle the application wants back — a whole one, fields and
all, so a handler is handed what it needs rather than one field's worth. Just
a class name is the short way of writing `{ _class = "…" }`.

Cancelling one that has already fired, or was never started, is `ok = false`
rather than a failure: it means the same thing either way.

## Nothing repeats on its own

A delay fires once. A handler that wants a heartbeat asks for the next one
itself — one line at the end of the handler that already ran.

Repeating would mean a timer outliving the reason it was started, which is
how a program ends up doing work nobody asked for and nobody can find.

## It does not hold the program open

On a machine a program ends at its last statement unless a module says it is
still serving. A pending delay does not say that: an application that wants
to stay up is serving something — a socket, a queue — and a timer is not that
thing. A program whose only module is this one ends with its delay unfired,
which is what it asked for by having nothing else to do.

## Where it works

**A browser today.** On a machine every handler answers an `Exception` saying
the half that would do the work is not built yet — a thread and the inbound
queue are exactly what it would take, and nothing has needed it. A module
that silently drops what it was asked to remember would be worse than one
that has not been written.

For wasm it is built as an archive linked into the program:

```bash
cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
```

Its page half is in [`web/host.mjs`](../../../web/host.mjs), with every other
browser module's.

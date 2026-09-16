# `timer` — a particle, later

In a browser and on a machine alike.

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

## On a machine

The same shape, from a thread: `Delay` answers at once and the thread
sleeps, then pushes `then` onto the program's inbound ring — with
`_delay_id`, the number `Delay` answered — and the handler runs on the
program's own thread when its loop next drains, never inside another
handler. `Cancel` marks the number so the push is skipped; a fired or
unknown one answers `ok = false`.

## Nothing repeats on its own

A delay fires once. A handler that wants a heartbeat asks for the next one
itself — one line at the end of the handler that already ran.

Repeating would mean a timer outliving the reason it was started, which is
how a program ends up doing work nobody asked for and nobody can find.

## A pending delay holds the program open

On a machine a program ends at its last statement unless a module says it is
still serving. A pending delay says that — a thread of this module is
asleep, and it must fire or be cancelled before the module can go — so a
program stays up until its last delay lands, and a host will not unload an
application until it has cancelled what it armed — `Cancel` wakes the
thread and waits for it to leave, so when it answers, nothing of the delay
is still running. In a browser the page is what holds things open, and the same
page is gone before it fires.

## Where it works

Both halves. In a browser the page's own timers do the waiting; on a
machine a thread of this module does (see "On a machine"). The machine
half is built like any native module:

```bash
cargo build --release        # -> target/release/libtimer.so
```

For wasm it is built as an archive linked into the program:

```bash
cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
```

Its page half is in [`web/host.mjs`](../../../web/host.mjs), with every other
browser module's.

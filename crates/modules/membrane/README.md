# `membrane` — an application's boundary when a host is holding it

A cell's membrane is where it touches whatever is outside. That is all this
is: the place a held application's traffic arrives, standing where its own
door would be.

```code
link "membrane.so" as door

emit Listen { } to door get l
```

## The vocabulary is `net_server`'s, word for word

```
Config { … } → ConfigResult { ok }
Listen {}    → ListenResult { ok, port, message }
Stop {}      → StopResult   { ok }
```

That is the whole design. Moving an application from running on its own to
being held is one word in its manifest — `net_server` becomes `membrane` —
and not a line of its genes. It still says "start listening"; what changes is
what that means.

## No socket, no thread, nothing pushed

Not a limitation — the point. Those three are exactly what stops an
application from being held:

- a door of its own has a thread that outlives the application
- a thread that outlives it cannot be unloaded
- so its memory never comes back when it is stopped

A membrane has none of them, so an application wearing one can be started and
stopped cleanly, and stopping it really does give its memory back.

## Held, this code does not run

A host answers `membrane` itself and hands the application a stand-in with
these same three handlers (see [`code_abi.h`](../../../src/code_abi.h) item
10). Its `Listen` registers the application for traffic rather than binding
anything, and the host's own door routes to it by name.

So what is left here is the standalone case, and there the honest answer is
that there is nobody to be held by:

```
emit Listen { } to door get l
assert not l.ok
| l.message — "no host: this application's door is a membrane…"
```

`Listen` says so rather than pretending. An application built to be held, run
on its own, finds out at the line where it asks to start — not later, in the
silence where its traffic should have been.

`Config` and `Stop` both answer `ok`, so a shutdown path written once works
either way.

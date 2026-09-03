# `net_server` — particles in, particles out, no protocol

The other half of [`net_client`](../net_client). Where
[`http_server`](../http_server) speaks HTTP — paths, methods, status codes —
this speaks nothing. It accepts a **particle**, hands it to the program's own
handlers, and sends back whatever they returned.

```code
link "net_server.so" as net

Ping { value } => {
    return Pong { value = value }
}

emit Config { port = 9000 } to net get _
emit Listen { } to net get l
assert l.ok
```

That is the whole program — **there is no keep-alive loop.**

## Handlers

```
Config { port?, host?, max_particle_bytes?, response_timeout_seconds? } → ConfigResult { ok }
Listen { }                                                              → ListenResult { ok, port, message }
Stop   { }                                                              → StopResult   { ok }
```

`Config` is optional — `Listen` uses the defaults below — but sending it
*after* `Listen` is an `Exception`: the socket is already bound. One `Listen`
per program; a second answers `ok: false`. `Stop` before `Listen` is an
`Exception`, since nothing is serving.

| `Config` field | Kind | Default | Meaning |
|---|---|---|---|
| `port` | Number | `0` | 0 asks the OS for a free one; `ListenResult.port` says which. A whole number in `0..=65535` |
| `host` | String | `127.0.0.1` | loopback unless you say otherwise (`"0.0.0.0"` opens it to the network) |
| `max_particle_bytes` | Number | `1048576` | a larger frame is refused with an `Exception` sent back to the sender |
| `response_timeout_seconds` | Number | `10` | how long a sender waits for the program before being answered by this module instead |

## Pushed into the program

**The particle the sender sent**, its `_class` intact, plus two fields this
module adds:

| Field | Meaning |
|---|---|
| `app` | the url's app segment, so a runtime hosting several apps can route on it. Empty when the url carried none |
| `_request_id` | this module's own bookkeeping — how an answer finds its way back. Underscore-prefixed like `_class`; ignore it |

Plus `Log { source, level, message }` and `Exception { source, message }`, the
language's **common particles** — same shape every other module pushes, so one
handler serves them all. A pushed class the program does not handle is
dropped, so a program that wants no diagnostics writes no handler.

The handler's **return value is the answer.** Returning nothing — no handler
for that class — sends `null` back, which is a real answer and tells the
sender plainly rather than leaving it to time out.

## How long the program lives

`Listen` starts a thread. While it is alive this module answers the ABI's
`code_module_serving`, and **the host keeps the program up for exactly that
long** — the same rule a JVM follows for a non-daemon thread. So the program
runs past `assert l.ok`, reaches the end of its statements, and goes on
serving. Idle it costs nothing: the runtime parks on its own queue and wakes
on a real particle, not on a poll interval.

`Stop { }` ends it — from a handler, which means a sender can ask the program
to shut itself down:

```code
Quit { } => {
    emit Stop { } to net get _
    return Bye { }
}
```

`accept()` blocks with no portable interrupt, so `Stop` opens a connection to
the server's own address to wake it; that connection is never served.

## No authentication, no authorization, no policy

Deliberately. A token check belongs where the user and their permissions can
be read, and that is a handler, in `code`. So this module carries an envelope
it never opens.

What makes that work is a **chain of handlers**, which the language already
allows — a handler may emit to another, as long as the call graph stays
acyclic (see `src/handlers.rs`):

```code
Impulse { token, app, particle } => {
    emit Decode { token = token } to jwt get claims
    if claims ∈ Exception { return Denied { reason = "bad token" } }
    emit Authenticated { user = claims.sub, particle = particle } to this get r
    return r
}

Authenticated { user, particle } => {
    | the permission check happens here, with the user in hand
    emit particle to this get r
    return r
}
```

The last link works because a particle held in a variable can be emitted:
which handler runs is decided by its `_class` at runtime, and the re-entry
guard catches the case where that names a handler already on the stack.

It is also why this module knows nothing about euglena: no manifest, no
projects directory, no per-app public-class list. The old `server` organelle
read all three because it *was* a euglena organelle. This is a `code` module,
and the policy it would have enforced is a program's to write.

## The wire

A four-byte big-endian length, then that many bytes of JSON:

```
[len: u32 BE][{"app": "…", "particle": { "_class": "…", … }}]
```

The answer comes back the same way, as the returned particle. Nothing about
it suggests a protocol to a reader, which is the point: this and `net_client`
are two ends of one pipe, not an implementation of somebody else's standard.
JSON because the language's value model *is* JSON's six kinds, so a particle
crosses without a translation layer to argue with.

**No TLS.** A server facing the public internet belongs behind something that
terminates it.

## Many in flight, answered one at a time

Every connection gets its own thread and its own slot, so a sender never waits
for the socket. Dispatch into the program stays serial regardless — the host
drains on one thread, and a handler may not re-enter another — so the program
works through them in order.

That split is deliberate: the socket does not block, and the language's
single-threaded guarantee is untouched. It is also why `_request_id` exists
rather than `http_server`'s single pending slot.

## Build

```bash
cd crates/modules/net_server
cargo build --release
# -> target/release/libnet_server.so
```

# `net_client` — send a particle, get a particle back

The other half of [`net_server`](../net_server). One handler, and two things
to give it: where to send, and what to send.

```code
link "net_client.so" as net

emit Send {
    url = "http://127.0.0.1:9000/ping-api",
    particle = Impulse { token = "…", particle = Ping { value = 1 } }
} to net get answer

assert answer ∈ Pong
```

## Handlers

```
Send { url, particle, timeout_ms? } → whatever the far side's handlers returned
```

| Field | Kind | Default | Meaning |
|---|---|---|---|
| `url` | String | — | `http://host:port/app`. Required |
| `particle` | Particle | — | sent as written, `_class` and all. Required |
| `timeout_ms` | Number | `10000` | connect, send and read deadline. A positive number |

The answer is the particle the far side's handler returned. `null` comes back
when nothing there handled the class — a real answer, not a timeout.

## The url names a host and an app, and nothing else

```
http://127.0.0.1:9000/ping-api
└─ scheme ┘└── host:port ──┘└ app ┘
```

No path beyond the app segment, no method, no query. There is nothing to
design: a particle already says what it wants by its class. The app segment is
optional — `http://host:port` is a program that serves only itself — and it
reaches the far side as a field, so a runtime hosting several apps can route
on it.

A url with a scheme other than `http://`, without a port, or with more than
one path segment is an `Exception` naming what was wrong.

## It does not build the envelope

Whatever particle the program hands over is what crosses the wire. A token
belongs *inside* that particle, put there by a handler that knows which token
to use — this module never looks.

That is the same division `net_server` keeps on the far side: authentication
and authorization are a program's business, because that is where a user and
their permissions can be read. These two modules are a pipe, not a policy.

## Failure is a value

A refused connection, a timeout, a malformed answer, a bad url, a `particle`
that is not a particle — all come back as an `Exception { source, message }`
the program can read. Never a dead program: a module may not end the
application, and this one has more ways to fail than most.

```code
emit Send { url = "http://127.0.0.1:1/x", particle = Ping { } } to net get r
assert r ∈ Exception
assert r.source = "net_client"
```

**`Send` blocks** while it waits, like `http_client`'s handlers do, and bounds
the block: `timeout_ms` defaults rather than waiting forever, because nothing
in the ABI can stop a module that blocks with no deadline.

No connection reuse — a particle is one exchange, and a pool would be state
this module would then have to invalidate.

## In a browser it does not answer

`Send` returns the far side's particle on a machine. In a page it cannot:
waiting for a reply means blocking, and blocking there freezes the reader —
no rendering, no clicks, nothing. So the browser half answers as soon as the
request is on its way, and the reply arrives later, as a particle, at the
program's own handlers:

```code
emit Send { url = "http://…/ping-api", particle = Ping { value = 41 } } to net get sent
assert sent.ok

Pong { value, _request_id } => { … }
Denied { reason, _request_id } => { … }
```

`sent.value` is the number the exchange is known by, and every reply carries
it back as `_request_id` — so two requests that both answer `Pong` can be told
apart without the far side having to help.

**Replies arrive in whatever order they come back**, not the order they were
sent.

A refused connection, a timeout, an answer that is not a particle: all of them
arrive the same way, as an `Exception` with the same `_request_id`. One place
to handle a failure, and it is where the answer would have been.

The two shapes are not an accident of the port. On a machine every module
answers — `jwt`, `mongodb`, `fs`, all of them — and in a browser everything
that waits comes back as a particle: `timer`, `router`, a click on a `dom`
node. This module looks like its neighbours on each side, which is the
consistency that matters to somebody writing an application.

**On a machine, mind where you call it.** `Send` blocks, and a handler that
blocks stops the whole program — no other particle is dispatched while it
waits. At the top level that costs nothing, since nothing else is running. In
a handler of a program that serves requests, it is a stall that only shows
under load.

## The wire

One POST, the body is the particle, the path is the app. See
[`net_server`](../net_server#the-wire) for the shape, and for why a framing of
our own had to go.

**No TLS.** `https://` is refused rather than sent in the clear; put a proxy
in front and send it `http://`.

## Build

```bash
cd crates/modules/net_client
cargo build --release
# -> target/release/libnet_client.so
```

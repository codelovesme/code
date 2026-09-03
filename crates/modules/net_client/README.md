# `net_client` — send a particle, get a particle back

The other half of [`net_server`](../net_server). One handler, and two things
to give it: where to send, and what to send.

```code
link "net_client.so" as net

emit Send {
    url = "euglena://127.0.0.1:9000/ping-api",
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
| `url` | String | — | `euglena://host:port/app`. Required |
| `particle` | Particle | — | sent as written, `_class` and all. Required |
| `timeout_ms` | Number | `10000` | connect, send and read deadline. A positive number |

The answer is the particle the far side's handler returned. `null` comes back
when nothing there handled the class — a real answer, not a timeout.

## The url names a host and an app, and nothing else

```
euglena://127.0.0.1:9000/ping-api
└─ scheme ┘└── host:port ──┘└ app ┘
```

No path beyond the app segment, no method, no query. There is nothing to
design: a particle already says what it wants by its class. The app segment is
optional — `euglena://host:port` is a program that serves only itself — and it
reaches the far side as a field, so a runtime hosting several apps can route
on it.

A url with a scheme other than `euglena://`, without a port, or with more than
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
emit Send { url = "euglena://127.0.0.1:1/x", particle = Ping { } } to net get r
assert r ∈ Exception
assert r.source = "net_client"
```

**`Send` blocks** while it waits, like `http_client`'s handlers do, and bounds
the block: `timeout_ms` defaults rather than waiting forever, because nothing
in the ABI can stop a module that blocks with no deadline.

No connection reuse — a particle is one exchange, and a pool would be state
this module would then have to invalidate.

## The wire

A four-byte big-endian length, then that many bytes of JSON. See
[`net_server`](../net_server#the-wire) for the shape and why it is not HTTP.

## Build

```bash
cd crates/modules/net_client
cargo build --release
# -> target/release/libnet_client.so
```

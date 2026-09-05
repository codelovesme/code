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
Impulse { token, particle } => {
    emit Decode { token = token } to jwt get who
    if not who.valid { return Denied { reason = "bad token" } }

    | The allow-list: the classes this program offers, and what each needs.
    if particle._class = "Ping" {
        emit DoPing { user = who.sub } to this get r
        return r
    }
    if particle._class = "Report" {
        if who.role ≠ "admin" { return Forbidden { } }
        emit DoReport { } to this get r
        return r
    }
    return Unknown { class = particle._class }
}

DoPing { user } => { … }
```

**Name the classes you offer; do not emit what arrived.** `emit particle to
this` would hand a sender the whole program: the class is theirs to choose, so
they could reach any handler in it — `Log`, `Exception`, anything internal —
and pick which one runs. The allow-list is what keeps this transport from
being a dispatch table into your program.

Two things make the shape work. A particle held in a variable *can* be
emitted, so the class it names is decided at runtime; and `particle._class` is
readable, so a program can decide what to do about it first. The caller
travels as a field on the emitted particle rather than in shared state,
because a handler defined in a linked gene cannot see that gene's own
top-level `let` — the gene's statements are a scope of their own while its
handlers are hoisted to the program.

It is also why this module knows nothing about euglena: no manifest, no
projects directory, no per-app public-class list. The old `server` organelle
read all three because it *was* a euglena organelle. This is a `code` module,
and the policy it would have enforced is a program's to write.

## The wire

One POST, the body is the particle, the path is the app:

```
POST /ping-api HTTP/1.1
content-type: application/json

{"_class": "Ping", "value": 1}
```

The answer comes back as the returned particle, with status 200 whatever it
says. A `Denied` is an answer, not a transport failure — the status line is
about whether the *door* worked. JSON because the language's value model *is*
JSON's six kinds, so a particle crosses without a translation layer to argue
with.

Nothing wraps anything, so `curl -d '{"_class":"Ping"}'
http://127.0.0.1:9000/ping-api` is a whole request.

**It used to be a framing of our own** — a four-byte length, then that many
bytes. Smaller and simpler to read, with one fatal property: a browser cannot
speak it. A browser opens no raw sockets, so an application in a page could
never reach a program, whatever else it could reach. HTTP costs a few hundred
bytes per request and buys the rest of the world with them: a proxy in front,
TLS terminated by something that already knows how, a request visible in
devtools, and `curl` when something is wrong.

**What did not change is the shape**, which is what this module is for. A
particle arrives, the program's handlers answer it, the answer goes back;
there is still no path to design and no method to choose. HTTP here is only
how the bytes travel — which is why this is still not `http_server`, whose
job is the opposite one.

**A browser has to be told it may read the answer.** `Config { allow_origin }`
sets that header, open by default: this door carries a token it never opens,
and the handler that reads the token is where "who is allowed to ask for
this" is decided. An origin check here would look like an answer to that
question without being one.

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

# `http_server` — HTTP requests, answered by a handler

The other half of [`http_client`](../http_client). The client asks the world
a question; this lets the world ask one of your program.

```code
link "http_server.so" as srv

Request { method, path } => {
    return Response { "status": 200, "body": "hi from $path" }
}

emit Listen { "port": 8080 } to srv get l
assert l.ok

loop {
}
```

## Handlers

```
Listen { port?, host?, max_body_bytes?, response_timeout_seconds? } → ListenResult { ok, port, message }
```

| Field | Kind | Default | Meaning |
|---|---|---|---|
| `port` | Number | `0` | 0 asks the OS for a free one; `ListenResult.port` says which |
| `host` | String | `127.0.0.1` | loopback unless you say otherwise |
| `max_body_bytes` | Number | `1048576` | a larger request body is refused with 400 |
| `response_timeout_seconds` | Number | `10` | how long a connection waits for the program before answering 504 itself |

One `Listen` per program: a second one answers `ok: false`.

## Pushed into the program

```
Request { method, path, query, body }
Log { source, level, message }
Exception { source, message }
```

`Log` and `Exception` are the language's **common particles** — same shape
`http_client` pushes, so one handler serves both. A pushed class the program
does not handle is dropped, so a program that wants no diagnostics writes no
handler for them.

## The answer is the handler's return value

There is no `Respond` particle and no request id. A `Request` is answered the
way every particle in this language is answered — by returning one:

```code
Request { method, path } => {
    if path = "/health" {
        return Response { "status": 200, "body": "ok" }
    }
    return Response { "status": 404, "body": "no" }
}
```

| Returned | Sent |
|---|---|
| `Response { status?, body?, content_type? }` | as written; `status` defaults to 200, `content_type` to `text/plain; charset=utf-8` |
| any other particle | 200 with an empty body — returning something is the program saying it handled the request |
| null, or no `Request` handler at all | **404** — nobody claimed it |
| nothing, for `response_timeout_seconds` | **504**, sent by the module |

This works because a pushed particle's answer crosses back through
`code_module_inbound_reply` (see `code_abi.h`), which is what this module was
the reason for. The alternative — a `Respond { id, … }` particle emitted back
— was built first and thrown away: it made every program carry an id it never
chose, to solve a correlation problem that is the module's, not the
program's.

## The decisions, and why

**One request at a time.** The accept loop handles a connection to completion
before taking the next. The program it serves is single-threaded: pushed
particles are dispatched one at a time by the host's drain, and a handler may
not re-enter another. Accepting concurrently would queue work the program
cannot start any sooner, and would let one slow client's request overtake
another's in the bounded inbound ring.

That is also why the pending request is a single slot rather than a map:
when an answer arrives there is exactly one request it can belong to. No
correlation, no id, nothing for the program to carry.

**A program cannot call its own server.** `emit Get … to http` blocks inside
`http_client`, and the drain that would deliver the `Request` only runs
between the program's own statements — so a self-request waits for a handler
that cannot start until the request finishes. A property of a single-threaded
program rather than a bug here, and the reason
`tests/http_server_module.rs` makes its requests from outside the process.

**No request headers.** `code_object` copies key *pointers*, not key strings,
so every field name in a value must outlive it — which is why `object()`
takes `&'static CStr`. Header names arrive at runtime. `http_client` is
missing *response* headers for exactly the same reason; both want an
owned-keys constructor in the ABI, which is a decision bigger than either
module.

**No TLS, no keep-alive, no chunked bodies, no HTTP/2.** The parser reads a
request line, the one header that says how many body bytes follow, and those
bytes. Everything else is what a reverse proxy in front of this is for, and
saying so is more honest than a half-implementation that looks like it
handles them.

**Loopback by default.** A module that opened a program to the network the
moment it was linked would be making that decision for its caller. Saying
`"host": "0.0.0.0"` is how a caller makes it.

**A JSON body needs no support here.** String interpolation renders any value
as compact JSON, in both output modes:

```code
let payload = { "ok": true, "items": [1, 2] }
return Response { "status": 200, "body": "$payload", "content_type": "application/json" }
```

## Deliberately not here

- **No routing.** `path` is a string and `if` is in the language. A router
  that earns its own module can be written in `.code`.
- **No JWT, no sessions, no static files.** Each is its own module, or its
  own twenty lines of program.
- **No graceful shutdown.** The program ends when something kills it, which
  is what `loop { }` means. The ABI has no shutdown call on purpose: a module
  that must be asked before the program may exit is a module that can hang it.

## Build

```sh
cargo build --release        # -> target/release/libhttp_server.so
```

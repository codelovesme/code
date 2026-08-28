# `net` — HTTP requests

The first module that reaches something outside the machine. `terminal`
writes to a stream, `math` and `strings` are pure; `net` is the one where the
outside world can say no.

```code
link "net.so" as net

emit Get { "url": "https://example.com" } to net get r
assert r.ok
assert r.status = 200
```

## Handlers

```
Get  { url, headers?, timeout_seconds?, max_body_bytes? } → HttpResponse
Post { url, body, content_type?, headers?,
       timeout_seconds?, max_body_bytes? }                → HttpResponse
```

`HttpResponse { ok: Boolean, status: Number, body: String }`.

`ok` answers **did a response arrive**, not did the server like the request.
A 404 or a 500 is a perfectly good response: `ok: true`, `status: 404`, and
the body the server sent. `ok: false` means there was no response to have a
status — refused, unresolvable, timed out, TLS rejected, or larger than
`max_body_bytes` — and then `status` is `0` and `body` carries the reason.

| Field | Kind | Default | Meaning |
|---|---|---|---|
| `url` | String | — | required; empty is an error |
| `body` | String | — | `Post` only, required |
| `content_type` | String | `application/octet-stream` | `Post` only |
| `headers` | Object | none | `{ "Accept": "text/plain" }`; values must be Strings |
| `timeout_seconds` | Number | `10` | whole request, connect included |
| `max_body_bytes` | Number | `1048576` | a longer response fails the request |

## Diagnostics

`net` also speaks first. Every request that gets a response pushes

```
Log { source: "net", level: "Info", message: "Get <url> -> <status>" }
```

and every request that never got one pushes

```
Exception { source: "net", message: "Get <url>: <reason>" }
```

into the program, dispatched to *its* handlers between top-level statements.

```code
Exception { source, message } => {
    emit Print { "value": message } to term
}
```

Two properties make this safe to send unasked. A pushed class the program
has no handler for is **dropped**, so a program that wants none of this
writes none of it and nothing changes. And the push is **additional, never
instead of** — the `HttpResponse` still carries the whole story, so checking
`ok` remains a complete way to use this module. Diagnostics buy you one
central place to handle failures instead of a check at every call site; they
are not the only way to learn about them.

A 4xx or 5xx logs at `Info` and does *not* raise an `Exception`: the server
answered, and what it answered is in `status`.

## The decisions, and why

**One particle per verb, not `Request { method }`.** Dispatch in this
language is already a `_class` switch — `code_module_dispatch` reads
`_class` and routes. A method field would make the module perform a *second*
switch on a string, re-implementing the dispatcher one level down. The verb
belongs where the language already looks for it.

**A failed request is a value, not an error.** `ok: false, status: 0` for a
refused connection, a DNS failure, a timeout. This is not a style
preference: the language has no `try`/`catch` and no catchable assert, so a
`code_runtime_error` here would end the program with *no construct able to
recover*. Every other module can honestly treat a bad emit as a bug. `net`
cannot — a network failing is normal operation.

`code_runtime_error` is therefore reserved for genuine misuse the program
could have avoided: a missing `url`, a `headers` value that isn't a String.
Those are bugs, and they abort like every other module's bugs do.

**`body` comes back as a String, never a parsed Object.** Tempting to add
`GetJson`, and the interpreter could do it for free — `parser::parse_expr`
exists precisely because this language's literal grammar *is* JSON. But
`runtime.c` only *serialises* JSON; it has no parser. `GetJson` would work
under `code run` and be impossible under `code build`, breaking the
run/build invariant, which is the hardest rule in the repo. Parsing belongs
in a future `json` module that can serve both backends.

**Timeouts and body caps have defaults, not just knobs.** A program with no
way to interrupt itself should not be able to hang forever or swallow an
unbounded download because the author forgot a field. 10 seconds and 1 MiB
are the defaults; both are overridable per request.

**Over the cap fails the request; it does not truncate.** Euglena's client
truncates in `Get` and aborts in `GetBinary` — this module aborts in both.
A truncated body is *silently wrong*: it is a well-formed String that looks
like the whole answer and isn't, and nothing downstream can tell. That is
the same failure the wasm number-formatting decision refused to trade for
(`docs/todo/wasm-fractional-number-text.md`), and the reasoning carries over
unchanged. `ok: false` with a message naming `max_body_bytes` is the honest
answer. `max_body_bytes` is a cap on what is *acceptable*, not a request to
cut.

**Synchronous.** `emit … get r` blocks until the response arrives, which is
what `emit` already means everywhere else. No callbacks, no futures, and
notably nothing from the still-unbuilt keep-alive loop — `net` needs none of
it.

## Deliberately not here

- **No `GetJson` / `PostJson`** — see above; wants a `json` module first.
- **No `GetBinary` / base64** — binary in a language whose only string is
  UTF-8 is a policy decision of its own, not a detail of this module.
- **No response headers.** Wanted, and blocked on the ABI rather than on
  effort: `code_object` copies key *pointers*, not key strings, so a value's
  field names must outlive it — hence `object()`'s `&'static CStr`. Header
  names arrive at runtime. Returning them needs an owned-keys constructor in
  the ABI, which is a decision bigger than this module.
- **No server.** Accepting connections needs continuous draining and a
  thread pushing inbound particles — the open half of
  `docs/todo/inbound-emissions-from-native-modules.md`. A separate module
  when that lands.

## Prior art

Modelled on `native-http-client` and `http-client` in the `euglena-language`
repo, which settled the per-verb shape, the `{ ok, status, body }` response,
and `headers`/`max_body_bytes`.

`Log`/`Exception` are carried over too, by a different route: this language
has no `base`, so they are pushed as inbound emissions and land on the
*program's* handlers rather than a linking module's. Settling that is what
made unhandled pushes drop rather than error — see
`docs/todo/inbound-emissions-from-native-modules.md`.

Two things are deliberately *not* carried over: `Sap` (a cell-lifecycle
concept — this language has modules, not organelles, and there the handler
is a no-op that logs), and the JSON variants.

## Build

```sh
cargo build --release        # -> target/release/libnet.so
```

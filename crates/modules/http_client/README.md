# `http_client` — HTTP requests

The first module that reaches something outside the machine. `console`
writes to a stream, `math` and `strings` are pure; this is the one where the
outside world can say no.

Named for half of a pair. It was called `net` until 2026-08-29; the rename
is what makes room for an `http_server` that is a separate artifact rather
than a second half bolted onto a module most programs link only to make
requests. `http` alone would have forced the server to be `http_server`
against a client called `http`, which reads as though one of them is the
real one.

```code
link "http_client.so" as http

emit Get { url = "https://example.com" } to http get r
assert r.ok
assert r.status = 200
```

## Handlers

Seven, one per HTTP method. Four carry no request body:

```
Get     { url, headers?, timeout_seconds?, max_body_bytes? } → HttpResponse
Delete  { same }                                             → HttpResponse
Head    { same }                                             → HttpResponse
Options { same }                                             → HttpResponse
```

and three do:

```
Post  { url, body, content_type?, headers?,
        timeout_seconds?, max_body_bytes? }                  → HttpResponse
Put   { same }                                               → HttpResponse
Patch { same }                                               → HttpResponse
```

`HttpResponse { ok: Boolean, status: Number, body: String }`.

A `Head` response carries no body by definition, so its `body` is the empty
string. That is HTTP's answer, not a special case here.

`Delete` is on the bodyless side. HTTP permits a body on it and forbids
nothing, but no defined semantics attach to one and servers disagree about
whether it even arrives — a field this module offered would be promising
something it cannot deliver.

`ok` answers **did a response arrive**, not did the server like the request.
A 404 or a 500 is a perfectly good response: `ok: true`, `status: 404`, and
the body the server sent. `ok: false` means there was no response to have a
status — refused, unresolvable, timed out, TLS rejected, or larger than
`max_body_bytes` — and then `status` is `0` and `body` carries the reason.

| Field | Kind | Default | Meaning |
|---|---|---|---|
| `url` | String | — | rendered as text; absent or unfetchable fails the request |
| `body` | String | — | `Post`/`Put`/`Patch` only |
| `content_type` | String | `application/octet-stream` | `Post`/`Put`/`Patch` only |
| `headers` | Object | none | `{ Accept = "text/plain" }`; values are rendered as text |
| `timeout_seconds` | Number | `10` | whole request, connect included; a non-positive value takes the default |
| `max_body_bytes` | Number | `1048576` | a longer response fails the request |

## Diagnostics

This module also speaks first. Every request that gets a response pushes

```
Log { source: "http_client", level: "Info", message: "Get <url> -> <status>" }
```

and every request that never got one pushes

```
Exception { source: "http_client", message: "Get <url>: <reason>" }
```

into the program, dispatched to *its* handlers between top-level statements.

```code
Exception { source, message } => {
    emit Print { value = message } to term
}
```

`Log` and `Exception` are the language's **common particles**, not names
this module invented — see "Common particles" in the root README. That is what
lets one handler in a program serve `http_client` and every other module
that reports, with no branching and nothing to change when a link is added.
It is meant to read as the reference implementation of that agreement:
common shape, `source` carrying its own name, extra detail added as fields
rather than as a private class.

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

Held up when the other five methods landed on 2026-08-29: seven arms in a
`match` on `_class`, and the only thing that distinguishes them is whether a
body comes along — which is ureq's own split too
(`RequestBuilder<WithBody>` against `WithoutBody`), so the module never
re-derives what the caller already said.

**A failed request is a value, not an error.** `ok: false, status: 0` for a
refused connection, a DNS failure, a timeout. This is not a style
preference: the language has no `try`/`catch` and no catchable assert, so a
`code_runtime_error` here would end the program with *no construct able to
recover*. Every other module can honestly treat a bad emit as a bug. This one
cannot — a network failing is normal operation.

And as of 2026-08-28 that is the *only* answer: nothing here ends the
program, because no module may. There is no validation pass either — an
absent `url` is null, which is simply a url that cannot be fetched, so the
message comes from attempting the request rather than from a guard. A
non-String `url` or header value is rendered rather than refused, so
`url = 42` reports `bad uri: 42 is missing scheme` instead of something
about types.

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
what `emit` already means everywhere else. No callbacks, no futures. Since
2026-08-29 that is also the language's answer to waiting in general: a
module blocks inside its own dispatch, and the runtime never sleeps on a
program's behalf.

## Deliberately not here

- **No `GetJson` / `PostJson`** — see above; wants a `json` module first.
- **No `GetBinary` / base64** — binary in a language whose only string is
  UTF-8 is a policy decision of its own, not a detail of this module.
- **No response headers.** Wanted, and blocked on the ABI rather than on
  effort: `code_object` copies key *pointers*, not key strings, so a value's
  field names must outlive it — hence `object()`'s `&'static CStr`. Header
  names arrive at runtime. Returning them needs an owned-keys constructor in
  the ABI, which is a decision bigger than this module.
- **No server.** A separate `http_server` module, which is what the name
  leaves room for. Accepting connections needs a thread pushing inbound
  particles, which stopped being the open half of
  `docs/todo/inbound-emissions-from-native-modules.md` on 2026-08-29 — so
  the blocker now is only that nobody has written it.

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
cargo build --release        # -> target/release/libhttp_client.so
```

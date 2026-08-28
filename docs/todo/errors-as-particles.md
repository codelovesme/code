# Errors become particles: nothing but the program itself may end the program

Decided 2026-08-28. This reverses a documented decision — the root README
still lists catchable asserts among the things the language deliberately
does not have, and `docs/todo/inbound-emissions-from-native-modules.md`
records `Exception` as explicitly unported. That text goes when this lands.

## The decisions

1. **A module may never bring the application down.** Hard constraint, not a
   guideline. Today `emit Get {} to net get r` prints
   `net: Get requires a 'url' field` and exits 1 — a module deciding the
   program dies. That must become `r = null` and a continuing program.
2. **A particle sent to a module with no handler for it is discarded**, and
   the emit yields null. (Already agreed separately; it is the same rule seen
   from the other side.)
3. **`assert <false>` stops being a program-ending error** and becomes,
   semantically, `emit Exception …` followed by `return` — the current
   handler unwinds and its caller carries on.
4. **At the top level of the entry program there is nothing left to unwind
   to**, so the program ends. That is accepted.
5. **Errors before the program runs may still be errors** — parse, verify,
   link, duplicate handler, cycles. Nothing here changes those.
6. Module authors are told the constraint, and a module that fails it does
   not get into the registry.

## What must not break

`tests/` holds **76 `fail_*` fixtures** and **144 fixtures using `assert`**,
and the harness decides pass/fail purely on a non-zero exit status. So:

> **A failed `assert` at the top level must still exit non-zero.**

This is not in tension with decision 4 — "the program ends" and "the program
ends *reporting failure*" are the same sentence here — but it has to be
written down, because an implementation that ends the program at status 0
would leave all 76 fixtures silently passing and remove the language's only
way to state that something must not work.

Migration check needed: fixtures that `assert` *inside a handler* rather
than at the top level change meaning under this design — the handler
unwinds, the caller continues, and the program may now succeed. **There are
13**, found by scanning for an `assert` after a `=> {`:

```
fail_handler_local_leak  handler_basic  handler_chain
handler_dispatch_by_class  handler_emits_core  handler_fields
handler_no_return  handler_outer_scope  handler_scope_is_top_level
inbound_basic  inbound_none_queued  inbound_overflow_drops_oldest
net_diagnostics
```

`fail_handler_local_leak.code` is the one to look at first: it is a `fail_`
fixture whose failure *is* an assert inside a handler, so under the new
semantics it would unwind and the program could exit 0 — a fixture that
silently stops testing what it was written for. These have to be re-stated
before the semantics change, not after.

## The two tensions the decisions do not yet resolve

### A. Can an unhandled `Exception` end the program?

The two goals pull opposite ways:

- If **yes**, a failed `assert` with no `Exception` handler ends the program,
  which is exactly what keeps the 76 fixtures working (none of them defines
  an `Exception` handler).
- If **no**, a failed assert deep in a handler is silently swallowed: the
  handler returns null, the caller continues with null, and nothing anywhere
  reports it. That is the silent-wrongness this repo has refused everywhere
  else.

But "yes" collides with decision 1: `net` pushes `Exception` today for a
refused connection, and `tests/net_unreachable.code` handles it nowhere and
passes. Under "yes" that program would die — a module ending the
application, which is the exact thing decision 1 forbids.

**Proposed resolution: `Exception` and diagnostics are different things.**

| Particle | Meaning | Unhandled |
|---|---|---|
| `Exception` | control unwound — something failed | ends the program, non-zero |
| `Log` (level `Error`) | a report; the caller already has the answer | dropped |

Under that split `net`'s refused connection is a `Log` at `Error`, not an
`Exception` — which is arguably what it always was, since the caller is
handed `ok: false` and has lost nothing. `Exception` is reserved for "the
work did not complete and control left early". `net_unreachable.code` keeps
passing, and a failed assert stays loud.

This needs confirming before any code is written; it changes what `net`
pushes.

### B. What does a module return when it refuses?

Decision 1 says `r = null`. Decision 6 says a module "may only return
`Exception` when there is a problem". Those are two different answers to the
same question. The likely reading, matching what `net` already does for
network failures, is: **return null *and* report** — the value says "nothing
came back", the pushed particle says why. Which particle it pushes depends
on A.

## Scope: everything that can end a program today

Gathered from source rather than memory, so the work has a definite edge.

**Ends the program before it runs — unchanged by this ticket (decision 5).**
Parse and lex errors; `duplicate handler`; handler cycles; link resolution,
circular links, ELF/ABI mismatch; `break`/`continue` outside a loop;
`link`/`export` inside a block; undefined variables under `code build`.

**The program's own logic — not covered by the decisions above.**
`assertion failed` (covered), `division by zero`, arithmetic on non-numbers,
comparison across kinds, non-boolean in `if`/`assert`/`and`/`or`/`not`,
negating a non-number, `.` on a non-object, `[]` on a non-container, `loop`
over a non-container, `emit` of a non-particle, a handler returning a
non-particle, undefined variable under `code run`.

Decision 3 names only `assert`. Whether `1 + "a"` should also become an
`Exception` is open — and it is the difference between "asserts are
catchable" and "the language has exceptions". Worth deciding deliberately
rather than by drift.

**Dispatch.** `to this` / `to core` with no handler still error (the program
addressed itself or the core; a wrong address is a bug). `to <alias>` with
no handler becomes null. An inbound push with no handler already drops.
A handler re-entering itself still errors.

**From inside a module — what decision 1 is really about.**
Every `code_runtime_error` call a module makes. `net` alone has six. The
host cannot police these: `code_runtime_error` is exported to modules from
`runtime.c` and calls `exit(1)`. Honouring decision 1 means either removing
it from the module-facing ABI or making it non-fatal, and either way every
existing module — `net`, `strings`, `math`, `terminal`, and the test
doubles — has to be rewritten to return rather than abort.

**System and resource — noted, to be discussed (decision 5's "later").**
`out of memory`; rendering a fractional number as text under
`--target wasm`; the `CODE_CHECK_LEAKS=1` exit-time abort; a native module
failing to load. Stack overflow is *not* on this list: deep-nesting
traversals became iterative on 2026-08-26 and `stress_deep_nesting.code`
holds that.

## Suggested phasing

Each phase is independently shippable and leaves the suite green.

1. **`to <alias>` with no handler yields null.** Self-contained, no ABI
   movement, no `Exception` semantics needed yet.
2. **Modules stop aborting.** Remove or defuse `code_runtime_error` in the
   module-facing ABI; rewrite `net`/`strings`/`math`/`terminal` and the test
   doubles to return null and report. Largest mechanical change; needs A
   answered first, since it decides what they report.
3. **`assert` becomes `Exception` + `return`.** Needs A answered, needs the
   handler-internal-assert fixtures migrated first, and touches both
   backends plus the `Flow` enum in the interpreter and the unwinding path
   in codegen.
4. **The rest of the program's own logic** (division by zero, type
   mismatches), if that is wanted — see the open question above.
5. **System and resource cases**, discussed one at a time.

## Constraint to hold throughout

Both output modes must agree on which programs fail, exactly as they do
today. The fixture harness only checks pass/fail, so a divergence here would
not be caught by anything currently in the suite — every phase needs its own
both-mode fixture.

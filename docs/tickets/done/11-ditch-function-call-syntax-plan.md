# T11 [PLANNING — DONE] — Retire `name(args)` function-call syntax; move built-ins to handlers

- **Priority:** High (architectural correctness — the language has no function
  concept, but the interpreter/codegen still carry one)
- **Type:** Planning only — decision recorded below; implementation is T12
- **Area:** `parser.rs`, `ast.rs`, `interpreter.rs`, `codegen.rs`, `wasm_module.rs`
- **Decision on record:** Code has **no user-defined functions** and no
  function value. Reusable/invocable logic exists only as **handlers**
  (particle dispatch via `emit X to target get result`). Native "function"
  concepts are explicitly not wanted going forward.

## Why this exists

An audit of the language found that the README documented first-class
functions (`(a, b) => {...}`, closures explicitly disallowed, `Function` as a
type-checkable value) as a **shipped, `[x]`-marked feature**. It never existed
in the parser, AST, or runtime — `Value` has no `Function` variant, and the
grammar has no `=>`-as-lambda syntax. The docs have been corrected (README's
"Functions" section removed; see the "docs: correct..." commit that shipped
alongside this ticket) to describe reality: handlers are the only invocable
unit.

That correction exposed a real leftover, though: `Expression::Call` — the
`name(args)` call-expression node — **does** exist and **is** wired end to end,
but only for two hardcoded built-in names. It's function-call machinery with no
function concept behind it, kept alive purely to serve two built-ins. This
ticket plans its deliberate retirement.

## Current state (exact inventory)

`Expression::Call { callee, args }` is implemented in three places, each
independently hardcoding the same two names:

1. **Parser** (`src/parser.rs`) — grammar accepts `identifier(args)` as a call
   expression.
2. **Interpreter** (`src/interpreter.rs:1202-1227`) — `eval_expr` matches on
   `callee` name: `"timestamp"` and `"length"`; anything else is
   `Unknown function: {name}`.
3. **Codegen** (`src/codegen.rs:4085`, `compile_call`) — the same two names,
   independently reimplemented in raw LLVM IR (~35+ lines just for
   `timestamp`).

There is also a **dead ABI slot** carried for the same reason: the `.wasm`
module descriptor still reserves `fns_ptr`/`fn_count` (function-export slot,
offsets 12/16) for backward compatibility, but `wasm_module.rs:152` notes the
host never reads it. The `.so` ABI (`code-abi` crate) already dropped
`CodeExportFn` — that side is clean.

## Direction (to decide, not implement, here)

Replace `timestamp()`/`length(x)` call syntax with the handler idiom already
used everywhere else in the language (see `tests/handler_bare.code`,
`tests/native_modules/test_math.c`): a particle triggers a handler, invoked via
`emit X{...} to target get result`. Native modules already follow exactly this
pattern (e.g. `Point{}` → `handle_point`).

Two concrete shapes, to choose between:

**Option A — explicit emit, no call sugar.**
```
emit Length { value = arr } to core get n
assert n = 3
```
Simplest, fully consistent with the rest of the language, but more verbose for
things users reach for constantly (`length`, `timestamp`).

**Option B — keep `name(args)` as sugar, desugar to an emit.**
`length(a)` parses to the same call syntax as today, but the parser/interpreter
treat it purely as syntactic sugar for
`emit Length { value = a } to core get <tmp>` — no function value, no
`Expression::Call` "callable" semantics, just a fixed rewrite. Preserves
ergonomics; keeps `Expression::Call` in the grammar but redefines what it means
(desugars to emit, not "invoke a callable").

Either way, `timestamp`/`length` (and any future built-in) become **handlers
dispatched through the same registry mechanism already used for native
modules** (`NativeHandlerInfo` / `NativeFnPtr` in `native_module.rs`), just
pre-registered "core" handlers with no `.so` to load — no new dispatch
mechanism needs inventing, only a way to seed the handler registry at
`Interpreter::new()` / `Codegen::new()` time instead of via `link`.

## Basic necessities to plan for (beyond `timestamp`/`length`)

The language currently has **zero** string manipulation and **zero** math
functions beyond `+ - * /`. Once built-ins move to the handler idiom, these are
the concrete candidates worth planning core handlers for (not deciding scope
here — just the inventory to plan against):

- **String:** substring, split, upper/lower case, trim, indexOf/contains,
  replace.
- **Math:** abs, floor, ceil, round, sqrt, min, max, pow; modulo (there is
  currently no `%` operator at all — separate from this ticket's scope, but
  the same "how do built-ins get exposed" question applies to whether modulo
  becomes an operator or a core handler).
- **Array:** the existing `length`; consider indexOf/contains as core handlers
  under the same scheme once decided.

## Explicitly out of scope for this ticket

- No native (Rust/C `.so`/`.wasm`) function-call surface is being introduced or
  kept — the direction is to remove `Expression::Call`'s "callable" semantics
  entirely, not to route it through native modules.
- Implementation of Option A/B, the core-handler registry seeding mechanism, or
  any specific new built-in — all deferred to follow-up tickets once the
  approach here is chosen.

## Decision (recorded)

**Option A — full retirement, no `name(args)` sugar survives.**

Rationale, in the owner's own words: *"fonksiyon mantigimiz yok dilde - eger
hala buna support veriyorsak bunu planli bir sekilde ditch etmeliyiz"* ("we
have no function logic in the language — if we still support it, we must
deliberately/systematically ditch it"). Sugar that still parses as
`name(args)` — even if it desugars to an emit under the hood — **is** the
call-shaped syntax being ditched; keeping it would recreate the exact
doc-vs-reality ambiguity this audit exists to close (README documented
function-call-shaped syntax as if it invoked real functions). `length(a)`
becomes `emit Length { value = a } to core get n`.

Concretely: `Expression::Call` is not "reduced to two built-ins" — it is
**removed**, since (per the current-state inventory above) it has no purpose
beyond those two hardcoded names. See T12 for the implementation plan.

## Effort

Planning only here — done. Implementation is T12.

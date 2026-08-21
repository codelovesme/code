# 24 — Native backend: implement constraint narrowing (or keep it a clean rejection)

- **Priority:** Medium
- **Type:** Correctness / interpreter-codegen parity
- **Area:** `src/codegen.rs`
- **Status:** Done (2026-08-09) — both phases shipped. Full runtime
  `Domain` lowering remains explicitly out of scope unless a real
  consumer shows up (see Phase 2's writeup for why); not tracked as a
  follow-up ticket.

## What happened

While designing T23, the owner raised a general principle: "interpreter ve
compiler aynı davranışları sergilemeli — birbirlerinden farklı olma
ihtimalleri dahi olmamalı" (the interpreter and compiler must behave
identically — there shouldn't even be a *possibility* of them diverging).
Investigating turned up a real, previously-unnoticed soundness hole, not
just a missing feature.

`codegen.rs`'s `Statement::Constraint` handling only ever implemented two of
the eight `ConstraintExpr` variants — `Equals` and `IsType`. Every other form
(`LessThan`, `GreaterThan`, `LessEqual`, `GreaterEqual`, `NotEquals`,
`MemberOf`, `Domain(Z/N/R)`) fell into a catch-all that — as of `705ba19` and
earlier — read:

```rust
_ => {
    // Other constraint forms (LessThan, GreaterThan, etc.) are not
    // directly representable in compiled output yet; silently accept.
    Ok(())
}
```

Verified concretely (both commands run against the same source file):

```
a > 3
a < 10
a = 15
```

- `code run`: `error: Contradictory constraints for 'a': domain is empty` (correct).
- `code build` (before this ticket's fix): compiled with **no error**, and
  the resulting binary **ran to completion, exit 0** — the `a > 3` / `a < 10`
  narrowing was silently dropped; only the final `a = 15` assignment
  survived codegen.

Same result for `a in Z; a = 1.5` (non-integer pin — interpreter rejects,
native silently compiled and ran).

This is a strictly worse category of bug than "unimplemented": the compiler
didn't refuse the program, it produced a binary whose observable behavior
*silently contradicts* what `code run` does on the identical source. No
existing test caught it because `tests/llvm_codegen.rs` only ever compiles a
small curated set of `.code` files (`basic_assignment`, `object_basic`,
`equal_numbers`, `negative_literal`, `core_handler_length`,
`object_equality`, `object_nested`, `native_link_*`) — none of which use
range/domain narrowing — and the interpreter-side constraint fixtures
(`constraint_domain.code`, `fail_pin_contradicts_narrowing.code`, etc.) were
never run through `code build` at all.

**Not everything diverges, for what it's worth** — checked directly:
single-assignment/reassignment enforcement (`a = 1; a = 2` at top level) *is*
correctly mirrored between the two backends today, matching error intent on
both sides. So parity effort clearly happened for that one rule; it just
never got extended to the narrowing family.

## Fixed — Phase 1 (`a59eb95`, shipped same session)

Changed the catch-all from silently accepting to rejecting with a clear
error naming the unsupported constraint and pointing at `code run`:

```rust
other => Err(format!(
    "'{}' uses a constraint form not supported by the native \
     backend yet ({}) — this compiles differently than `code run` \
     would interpret it, so it's rejected rather than silently \
     ignored. Use `code run` for constraint narrowing beyond `=` \
     and type checks.",
    variable, other
))
```

Verified: both repro cases above now fail to compile with that message;
ordinary supported programs (`Equals`/`IsType` only) are unaffected; full
regression sweep green. Two regression tests added to
`tests/llvm_codegen.rs` (`build_rejects_unsupported_narrowing`, covering both
the range form and the `in Z/N/R` form). **This closes the soundness hole —
`code build` can no longer silently disagree with `code run`** — but it does
so by refusing the program, not by implementing the semantics.

## Phase 2 (this ticket, not started) — actually implement it

Whether Phase 2 happens at all should probably be revisited once T23 lands,
since T23 is exactly what would make range/set narrowing something people
actually write (today's corpus audit — 30 real apps, 15,521 lines — found it
essentially unused). Two shapes to choose between when picked back up:

1. **Full parity**: lower `Domain`'s `intersect`/contradiction-detection
   machinery into LLVM IR — runtime range checks, a real `IntegerRange`/
   `RealRange`/`ValueSet` representation on the native side, trapping on
   contradiction the way the interpreter errors. This is real work — the
   native side has never had *any* domain representation, only bare values.
2. **Scoped parity**: only support the narrowing forms T23 actually
   introduces (Set literals, `∈`/`in` domain-borrowing) rather than the full
   general range-constraint family, since that's the part with an actual
   consumer. `loop a { }` (T23) would need this either way if it's ever
   meant to work in compiled programs, not just the interpreter.

Either way, this ticket's Phase 1 fix means the choice is no longer urgent —
the worst case (silent divergence) is already closed off.

## Phase 2 — implemented (2026-08-09): scoped to compile-time constants

Chose neither of the two shapes above exactly. Full `Domain` lowering into
LLVM IR is real, substantial work for a construct the corpus audit found
"essentially unused" — not justified without an actual consumer. But
"scoped to what T26 introduced" (Set/Schema `∈`) would mean giving the
native side a first-class Set/Schema *value* representation, which is its
own large undertaking or the reason `∪`/`∩`/`SetLiteral`/`LoopDomain` are
all already cleanly rejected by codegen (T26+) — not something this
ticket should absorb either.

Instead: **range/domain narrowing (`<`, `>`, `≤`, `≥`, `∈ Z/N/R`) is
verified entirely at compile time when every operand is a constant
literal** — no runtime `Domain` representation at all, nothing added to
the compiled binary. `b > 3; b < 10; b = 15` now fails to compile with
the exact same message `code run` produces (`Contradictory constraints
for 'b': domain is empty`), instead of the Phase 1 blanket "not
supported." A *consistent* constant range (`b > 10; b < 20; b = 15`)
now compiles and runs correctly, holding `15` — real support, not just a
stricter rejection.

Mechanism: `ConstRange` (`src/codegen.rs`), a small compile-time-only
accumulator — deliberately not reusing `runtime::Domain`, since this has
no runtime representation to share and duplicating the ~10-line
lower/upper-bound-merge logic keeps codegen decoupled from the
interpreter's domain machinery. Scoped identically to `type_annotations`
(`Vec<HashMap<String, ConstRange>>`, pushed/popped with every scope).
Every accumulated bound is checked for a still-satisfiable range
(including "does an integer-only range still contain an integer" —
`a ∈ Z; a > 3; a < 4` has none) on every update, and again against a
constant `=` pin whenever one is applied, in either order.

Anything not provably constant still falls back to the Phase 1
rejection, unchanged — a non-constant operand, or a range constraint
against a variable already pinned to a non-constant expression (no
compile-time value exists to check the new constraint against; silently
accepting it would reintroduce the exact divergence this ticket exists
to close).

**Found and fixed along the way, not part of the plan going in:** `≠`
turned out to be a documented-but-easy-to-miss special case —
`constraint_narrowing_domain`'s `NotEquals` arm evaluates its operand
but always returns `Domain::Any`, an intentional, pre-existing,
out-of-T26-scope limitation (real exclusion narrowing was never
implemented). A first pass at this ticket gave codegen *real* `≠`
exclusion — stricter than the interpreter, and therefore a new
divergence in the opposite direction (`code build` rejecting `c ≠ 5;
c = 5` while `code run` silently accepts it). Caught by testing the
rejection path, not just the acceptance path. Fixed by making codegen's
`≠` handling match the interpreter exactly: compile and discard the
operand, no narrowing effect — parity means matching current behavior,
not guessing which side is "more correct." Worth a separate ticket if
the interpreter's `≠` gap itself is ever picked up.

Regression coverage: `build_rejects_contradictory_constant_range` (the
same two Phase-1 fixtures, now asserting on the precise contradiction
message rather than just "fails somehow"), `build_supports_consistent_
constant_range` (`tests/native_const_range_ok.code` — a consistent
`</>`/`=` range, a consistent `∈ N`/`</=` range, and a `≠`-then-`=`
parity case — compiles, links, and runs to completion). Full regression
sweep green throughout.

## Explicitly out of scope

- Auditing *every* other interpreter/codegen behavior pair for parity
  (block-scoping rules, `loop`/`yield` semantics, string interpolation,
  native-module linking, etc.) — this ticket is scoped to the constraint-
  narrowing family specifically, since that's what the investigation
  actually found diverging. A broader audit could be its own ticket if the
  owner wants one.

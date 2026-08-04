# 23 — First-class Set domain: `⦃…⦄` literals, domain-borrowing `∈`, and `loop <var> {}` possibility enumeration

- **Priority:** Medium
- **Type:** Language design / core semantics (interpreter only, see "Out of scope")
- **Area:** `src/parser.rs`, `src/ast.rs`, `src/runtime.rs`, `src/interpreter.rs`, `src/environment.rs`

## Context

While investigating whether Code is "really" constraint-based (owner's question,
2026-08-04), a corpus audit of 30 real apps (96 `.gene.code` files, 15,521
lines, in `codelovesme/euglena-language`) found that progressive domain
narrowing (`a > 3; a < 10`) is never actually used in practice — every real
program uses `=` as plain single-assignment. Two real bugs were found and
fixed along the way, landing directly on `main` (no ticket, small enough to
ship same-session):

- `f1795c9` — pinning a narrowed variable with `=` didn't check prior
  constraints (`b > 3; b < 10; b = 15` silently succeeded instead of
  contradicting).
- `63fe8e3` — `in Z/N/R` collapsed the integer/natural/real distinction to a
  generic `TypeDomain(Number)`, so range narrowing could never actually
  collapse to a concrete integer; and `∈ Z/N/R` (the symbol form) didn't
  reach that logic at all, silently treating `Z`/`N`/`R` as an undeclared
  particle type. Both fixed; `intersect()` gained real `IntegerRange` combinators
  and `∈`/`in` now agree on `Z/N/R`.

That second fix led to a longer design conversation (2026-08-04/05) about
whether the constraint-narrowing feature — real but unused — could become
something people actually reach for. This ticket is the result: named,
reusable finite sets: with domain-borrowing and possibility-enumeration.

## Design (agreed with owner 2026-08-05)

**Set literal — `⦃1, 2⦄`, not `{1, 2}`.** `{}` is taken by object literals
(`{ field = value }`); reusing it for sets would require the parser to
disambiguate object-vs-set by peeking for `=` inside — decided against.
`⦃`/`⦄` (U+2983/U+2984, "white curly bracket") was chosen instead: reads as
set notation, zero grammar collision, and matches the project's existing
convention of reaching for math symbols for set-theoretic constructs
(`∈ ∉ ∪ ∩ ≤ ≥ ≠`) — logical operators (`and`/`or`/`not`) deliberately stayed
keywords instead (decided 2026-07-27), so the split is: keywords for logic,
symbols for set/comparison ops.

**`A = ⦃1, 2⦄` produces a resolved `Value::Set`.** This is the key
distinction the owner drew out over several rounds: a set is not
automatically a possibility space. `A = ⦃1, 2⦄` is a *definition* — A is
determinately the set `{1, 2}`, immediately usable: loopable, passable to a
handler, storable as an object field — the same shape as `nums = [1, 2, 3]`
resolving to a concrete `Value::Array`, just unordered and deduplicated
instead of ordered-with-duplicates. This needs a new `Value::Set(Vec<Rc<Value>>)`
(or equivalent dedup'd representation) variant, parser support for `⦃…⦄` as
a genuine `Expression` (not just constraint-statement sugar — it must be
usable on the right of `=`), a `Display` impl, and `∈ Set` type-name support
in `value_matches_type_expr`.

**`a ∈ ⦃1, 2⦄` (or `a ∈ A`) narrows a *scalar*'s domain — stays unresolved.**
Unlike `=`, `∈` never produces a resolved value here — `a` remains "one of
these, not yet known which" until further narrowed to a single element,
using the existing `Domain::ValueSet` machinery (already implemented,
already gives the "possible values: {…}" diagnostic from the `=`-pin fix).
The new part: `a ∈ A` (RHS is an identifier naming another variable, not a
literal) must work even when `A` itself was never "resolved" in the scalar
sense — because per the rule above, `A` bound via `=` to a set literal *is*
resolved (as a `Value::Set`), so `eval_expr` on `A` already works today.
Handling is one code path: `ConstraintExpr::MemberOf` evaluates the RHS to a
`Value` and accepts both `Value::Array` (existing) and `Value::Set` (new),
extracting elements either way. No domain-borrowing-from-an-unresolved-
variable case is needed under this design, since sets are never unresolved —
only scalars narrowed via `∈` are.

**`loop a { }` — new grammar, enumerate a scalar's own domain in place.**
No `over` clause; `a` is both the thing being enumerated and, inside the
block, the per-iteration binding. Three points confirmed with the owner:

1. **Finiteness is required.** `a`'s domain must be enumerable —
   `Domain::ValueSet` or a *bounded* `Domain::IntegerRange`. An unbounded
   `IntegerRange` or *any* `Domain::RealRange` (uncountable even when
   bounded — infinitely many reals between 0 and 1) must be a compile/runtime
   error. This is not a style preference — it's required to preserve the
   language's existing "total work is always statically bounded" guarantee
   (the same reason there's no `while`/counter-loop/recursion — decided
   2026-07-31, "`loop <var> over <N-element array>` is sufficient
   iteration").
2. **Block-scoped shadow, confirmed non-resolving.** Inside `loop a { }`,
   `a` is a fresh binding scoped to the loop body — mirrors the already-
   verified `if`-block scoping (a name defined inside `if { }` does not
   leak to code after the block; tested directly, see conversation). The
   *outer* `a` is **never resolved** by this loop — after `loop a { }`
   finishes, `a` is exactly as unresolved as it was going in. This was an
   explicit, deliberate choice (not a default) — the owner confirmed
   visiting every possibility is not the same as picking one.
3. **`get` collection works the same as today's `loop x over y get result`.**
   `loop a get results { yield a * 2 }` collects yielded values across
   iterations, identical mechanism to the existing form.

## Known adjacent gap to fix alongside this

`Domain::ValueSet` doesn't narrow further when intersected with another
domain kind yet — verified:
```
a in [1, 2, 3]
a > 1
assert a = 2
```
→ stays stuck unresolved ("possible values: {1, 2, 3} and _ > 1") instead of
collapsing to `{2, 3}`. Same class of bug as the `IntegerRange` one fixed in
`63fe8e3` (`intersect()` has no `(ValueSet, RealRange)` / `(ValueSet,
IntegerRange)` / `(ValueSet, ValueSet)` arms, so it falls into the generic
`Intersection` wrapper and never actually recomputes the narrower set).
Needs the same treatment so the "same as assignment" analogy holds
consistently across every domain kind, not just integers. Natural to land
together with `Value::Set` since both touch the same `intersect()` code.

## Explicitly out of scope for this ticket

- **Native/LLVM codegen.** Constraint narrowing beyond `Equals`/`IsType` was
  never implemented in `codegen.rs` (confirmed: `grep -n
  "ConstraintExpr::" src/codegen.rs` only matches those two) — this ticket
  stays interpreter-only, consistent with that existing split.
- **Removing `in`.** Current split: `in` stays the operator for array-value
  membership (`a in someArrayVariable`, `Value::Array` source); `∈` covers
  type/domain membership and now set-value membership (`Value::Set`
  source). Whether to eventually unify further was raised mid-conversation
  but not decided — revisit separately if it comes up again.
- **Deep-equality edge cases for `Value::Set` dedup** (e.g. a set of Objects
  or Arrays) — verify `values_equal` (already used for `Domain::Exact`
  comparison) covers this correctly during implementation; not expected to
  need new logic, but not yet explicitly tested.

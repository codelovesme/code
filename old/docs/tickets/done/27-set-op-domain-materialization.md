# 27 — `∪`/`∩` materialize an unresolved-but-finite domain

Status: Implemented and shipped (2026-08-05).

## The gap

`b ∈ Z; b > 3; b < 6` narrows `b` to the domain `{4, 5}` — by the guide's
own description of the language ("a variable holds a domain, the set of
values it could still be"), `b` already denotes that set. But `∪`/`∩`
only ever operated on resolved `Value::Set`/`Schema`/`Union` values.
Referencing `b` in `a ∪ b` went through ordinary expression evaluation,
which requires `b` pinned to one concrete value first — so `a ∪ b` failed
with the generic "does not have a definite value yet" error, even though
nothing about `∪` actually needed `b` pinned to a single element.

Found via a playground example (`a = 5; b ∈ Z; b > 3; b < 6; c = a ∪ b`)
that a user expected to work and reasonably argued should.

## The fix

`Domain::finite_candidates()` (already the engine behind `loop <var> {
}`, T26 Phase 1) is now defined for every domain kind that's actually
enumerable, not just `ValueSet`/bounded `IntegerRange`/`Exact`:

- `Union`: concatenate every member's own finite candidates.
- `Schema`: the Cartesian product of each field's finite candidates, one
  `Object` per combination (fields sorted by name for determinism, since
  the schema itself is a `HashMap`).
- `Intersection`: enumerate from whichever part is finite on its own,
  then filter by every other part (reusing the same
  `intersect(Exact(v)).is_empty_domain()` containment check the
  `Union`+`Exact` narrowing arm already used).

`RealRange` (uncountable even when bounded), an unbounded `IntegerRange`,
and a bare builtin `TypeDomain` (`Number`, `String`, …) still correctly
error — no change there.

`Expression::Binary` evaluation now routes `∪`/`∩` operands through a new
`eval_set_operand` instead of plain `eval_expr`: a bare identifier that
isn't resolved yet, but whose domain is finite, gets materialized into a
`Value::Set` via `finite_candidates()`. Every other operand shape (already
resolved, not a bare identifier, or an infinite domain) falls through to
ordinary evaluation unchanged — including every other operator (`+`,
comparisons, …), which still requires resolution exactly as before. The
variable itself is never resolved by this, before or after — same
"outer variable untouched" invariant every other T26 block-scoped
narrowing construct keeps.

Because both `∪`/`∩` and `loop <var> { }` now share the same
`finite_candidates()`, extending it to `Schema`/`Union` also made
`loop <var> { }` work over object-schema and discriminated-union domains
for free — that was never possible before this ticket.

## Verification

New fixtures: `tests/set_op_domain_materialize.code` (scalar `IntegerRange`
and `ValueSet` domains, both operands unresolved, `∩` narrowing to the
actual overlap, the variable still usable unpinned afterward),
`tests/set_op_domain_materialize_schema_union.code` (`Schema` and `Union`
domains, plus the free `loop` win), `tests/fail_set_op_domain_unbounded.code`
(an unbounded domain still errors, doesn't hang). Full regression sweep —
workspace build, `cargo test --workspace`, the `.code` suite (166/166),
`docs/examples/run.sh`, wasm build + smoke test, `code fmt --check` — all
green before and after.

Also fixed while in the area: `docs/guide.html`'s Loops section and a
matching test-fixture comment both overclaimed "every program's total
work is statically bounded" — false since `LoopInfinite` (`loop { }`,
runs until an explicit `break`) has existed since before this session's
work. See the guide commit for that correction; unrelated to this
ticket's actual change, just found and fixed alongside it.

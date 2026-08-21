# 29 — `∈`'s right side is never rejected: any value denotes itself as a set

Status: Implemented and shipped (2026-08-05), same day as T28.

## The gap

T28 made `∈`'s right side accept a general expression, not just
type-shaped syntax — but it still errored when that expression resolved
to anything other than `Set`/`Schema`/`Union`:

```
p ∈ { k = 1 }
```

→ `Constraint '∈'/'in' requires a Set, Array, Schema, or Union, got Object`.

This is because `{ k = 1 }` — every field `=`, no `∈` fields — resolves
as a plain `Object` value, not a `Schema` (T26 Phase 2's rule: a
constrained field is what makes a literal a schema; all-`=` keeps its
pre-T26 meaning of a concrete resolved object). A concrete `Object` isn't
`Set`/`Schema`/`Union`, so the existing `∈` narrowing code rejected it
outright.

Raised as a direct follow-up to T28: if the whole point is "everything is
a set" (T26's founding claim, and T27/T28 both leaned on it), then a
concrete value like `{ k = 1 }` should be exactly as valid on `∈`'s right
side as `Number` or a bound `Set` — it just denotes the *singleton* set
containing itself, `{ {k=1} }`. There's no principled reason a Set/Schema/
Union get to be "sets" while every other value doesn't; restricting `∈`'s
right side to three specific `Value` variants was an implementation
artifact (only those three had *domain conversions written for them*),
not a deliberate semantic boundary.

## The fix

New `runtime::value_as_membership_domain(container: &Value) -> Domain`,
never fails: `Array`/`Set` → `ValueSet` of their elements (preserving the
pre-existing "legacy convenience" that array/set membership checks
against elements, not the container as one opaque unit), `Schema` →
`Domain::Schema`, `Union` → `Domain::Union`, and *anything else* →
`Domain::Exact(that value)` — the singleton-set reading. All three `∈`
call sites (the statement-level `ConstraintExpr::MemberOf` narrowing,
T28's `Expression::MemberOf` boolean check, and the bound-capitalized-
name path in `value_matches_type_expr`) now go through this one function
instead of each separately matching on `Set`/`Schema`/`Union` and
erroring otherwise. `ConstraintExpr::MemberOf` in particular got
substantially simpler — it's `Ok(value_as_membership_domain(&val))`,
full stop, no error arm at all anymore.

Practical effect: `p ∈ { k = 1 }` now narrows `p` to exactly that object
(same as `p = { k = 1 }` would); `n = 5; assert 5 ∈ n` holds and
`assert 1 ∈ n` doesn't, same as `n = 5; assert 5 = n` would. Every
existing Set/Schema/Union/Array behavior (T26, T27, T28) is unchanged —
this only replaces what used to be an error with a well-defined result.

## Verification

`tests/fail_member_of_non_set_container.code` (T28's "must error"
fixture) no longer describes real behavior and was replaced by
`tests/member_of_singleton_scalar.code` (the new passing behavior — both
the expression form and the exact `p ∈ { k = 1 }` statement form from the
bug report) and `tests/fail_member_of_singleton_mismatch.code` (a
singleton mismatch still correctly fails, `n = 5; assert 6 ∈ n`). Full
regression sweep — workspace build, `cargo test --workspace`, the
`.code` suite (169/169), `docs/examples/run.sh`, `code fmt --check` —
green before and after.

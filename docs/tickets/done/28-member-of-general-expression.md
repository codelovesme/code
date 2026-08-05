# 28 — `∈`/`∉` as a boolean expression accepts a general right side

Status: Implemented and shipped (2026-08-05).

## The gap

`assert 1 ∈ c` (where `c = p ∪ q`, a resolved Set) failed to even parse:
`found "c" but expected one of "-", "\""`. Expression-level `∈`/`∉`
(`Expression::TypeCheck`) only accepted a syntactically type-shaped right
side — `type_name()` requires the first character to be uppercase, so a
lowercase variable name like `c` could never match, and the parser fell
through trying a quoted string-literal type instead, hence the odd
expected-token list.

Raised directly against T27 (`∪`/`∩` domain materialization, ticket 27):
having just made `a ∪ b` treat an unresolved-but-finite domain as a set,
the natural next question was "is 1 an element of the result" — and `∈`
as a boolean expression couldn't ask that at all unless the container
happened to be a capitalized name. The counterargument (mine, initially)
that `∈`'s right side is "a type" was wrong on the language's own terms:
T26 already made types and sets the same concept (`unified set-based
semantics`) — `Number`, `String`, and a custom `type K {...}` were only
ever on the right side of `∈` *because* they denote sets. A variable
holding a `Set`/`Schema`/`Union` value denotes exactly the same kind of
thing and was arbitrarily excluded by a syntactic accident (capitalization
being used as a proxy for "this is type-shaped").

## The fix

New `Expression::MemberOf { expr, container, negated }`, parsed as a
fallback alongside the existing `Expression::TypeCheck` in the
`membership` parser tier: `type_expr_parser()` is tried first (unchanged
— `x ∈ Number`, `x ∈ "Success"` parse exactly as before), and only when
that fails does the right side fall back to a bare identifier
(`x ∈ c`). Deliberately *not* a full recursive `expression` fallback —
`membership` sits inside `expr`, which is reused dozens of times through
the grammar; a first attempt embedding `relational.clone()` there
stack-overflowed on every single parse (same class of bug as the T26
Phase 1 `∩`/`∪` precedence-tier postmortem — folding into an existing,
already-deep tier a second time is the trap, not adding a genuinely new
one). A bare identifier covers the actual need; anything more exotic
(`1 ∈ (a ∪ b)`) needs a named intermediate (`t = a ∪ b; assert 1 ∈ t`).

Evaluation (`Expression::MemberOf` in `eval_expr`) resolves both sides,
converts the container to its union-of-domains form via the existing
`value_to_union_members` (the same helper `∪`'s eval_binary arm already
uses), and checks membership via `Domain::intersect(Exact(val))` against
each alternative — reusing the core engine again rather than writing new
containment logic. A container that isn't a Set/Schema/Union (a plain
Number, say) is a clear runtime error, not a silent `false`.

`value_matches_type_expr`'s `TypeExpr::Named` arm got the same treatment
for the *parses-as-a-type-name* path: a capitalized name that turns out
to be a bound Set/Schema/Union variable (`assert 1 ∈ C`) is checked the
same structural way, instead of falling through to `_class` matching
(which would silently mean "is this object's class literally named
`C`" — always false for a Set). This mirrors the identical precedent
already established for the *statement*-level `x ∈ K` constraint form
(`constraint_narrowing_domain`'s `IsType` arm, T26 Phase 2) — the
expression form had simply never received the same fix.

Native codegen: both `Expression::MemberOf` sites (`compile_expr`,
`infer_expr_type`) get the same explicit T-ticket-referencing rejection
already established for `SetLiteral`/`LoopDomain`/`∪`/`∩` — interpreter-
only for now, `code run` required.

## Verification

New fixtures: `tests/member_of_value_expr.code` (Set/Schema/Union
containers, both `∈` and `∉`, the existing type-name form unaffected),
`tests/fail_member_of_non_set_container.code` (non-set container errors
cleanly). Full regression sweep — workspace build, `cargo test
--workspace`, the `.code` suite (168/168), `docs/examples/run.sh`, wasm
rebuild + smoke test (including the exact `1 ∈ c` scenario against the
actual compiled wasm, not just the native interpreter), `code fmt
--check` — all green before and after. Also manually reproduced and
fixed the stack-overflow regression from the first (rejected) approach
before it ever reached a commit.

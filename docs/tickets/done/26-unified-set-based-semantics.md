# 26 — Unified set-based semantics: variables are constraint sets, types are sets, `=`/`∈`

- **Priority:** Medium (foundational / large — a language-model redesign, not a feature)
- **Type:** Language design / core semantics
- **Supersedes:** [T23](23-set-domain-and-possibility-enumeration.md) (set literals + `loop`), [T25](25-partially-resolved-objects.md) (objects with unresolved fields) — both fold into this unified model.
- **Status:** Design locked (2026-08-05), all three core decisions resolved. **All phases (1: value-sets, 2: object-schemas, 3a: discriminated unions, 3b: flow-sensitive narrowing) implemented and shipped (2026-08-05)** — see "Implementation plan" for what landed and real bugs/infra issues caught and fixed along the way in each. Ticket complete.

## The model

The owner's thesis, developed and stress-tested across many rounds: **there is
no "type" vs "value" distinction, and no separate type-checking mechanism
alongside set membership — they are the same operation.** A variable always
holds a *set of possible values* — its domain. `Number`, `String`, a
hand-written `{1, 2}`, and an object schema are all just sets, some named,
some infinite, some finite. Sometimes a variable's domain is a singleton, and
then it reads as an ordinary value.

**One rule, no exceptions, no special cases per domain kind:**

```
x = v      ≡      x ∈ { v }      ≡      domain(x) := domain(x) ∩ { v }
x ∈ S      ≡                            domain(x) := domain(x) ∩ S
```

`=` is not a different operation from `∈` — it's `∈` where the right side has
been wrapped in a singleton set. Every constraint, on every kind of value,
narrows by intersection. There is no second, different rule for "type
checking" — `k ∈ String` and `1 ∈ {1, 2}` are the exact same operation,
because `String` is just an (infinite) set, same as `{1, 2}` is a (finite,
literal) one. **Earlier drafts of this ticket described `∈` as having two
modes ("type membership" vs "set membership") — that was wrong and is
retracted; there is one mode.**

### Worked example (validated across several rounds, including a caught
### self-contradiction that fixed itself once the rule was applied uniformly)

```
KK = String                          -> KK now names the set String does
K  = { k ∈ KK, m ∈ Number }          -> K names an object-schema set
mm ∈ K                               -> domain(mm) ⊆ K (open membership, unresolved)
mm = { k = "dfdfdf", m = 3 }         -> domain(mm) := domain(mm) ∩ {this object}
                                         "dfdfdf" ∈ String and 3 ∈ Number, so this
                                         object IS a member of K -> intersection is
                                         non-empty -> mm resolves. No contradiction.
```

vs. the refuted case:

```
mm = { k = "x", m = { 1, 2 } }       -> {1,2} ∉ Number (it's a set, not a member
                                         of Number) -> this object is NOT a member
                                         of K -> domain(mm) ∩ {this object} = ∅
                                         -> contradiction, rejected.
```

This works with zero special-casing because `{k ∈ KK, m ∈ Number}` and
`{k = "dfdfdf", m = 3}` are both just sets (one a predicate/schema, one a
literal singleton), and intersecting them is the same `∩` used everywhere
else.

### Constraints on the same variable always intersect

`e ∈ X; e ∈ Y` ⟹ `domain(e) = X ∩ Y`, unconditionally — this is exactly what
the interpreter's `apply_constraint`/`intersect` already does, extended to
every domain kind (value-sets, object-schemas, `Z/N/R`, ranges) uniformly
instead of only numeric ranges.

### Types are sets; `type` is (mostly) sugar

`type ABC { klm ∈ String }` names the set of all objects whose `klm ∈ String`.
`ABC = { klm ∈ String }` is the same set-naming. `type` is kept for now
because it *also* attaches predefined properties/metadata to members (e.g. the
`_class` tag) — removing it is deferred, not rejected.

Construction `abc = ABC { klm = "x" }` produces a specific member. The
resulting relationship `abc ∈ ABC` holds and is materialized concretely as the
member's `_class` field — verified in the current implementation
(`assert abc ∈ ABC` and `assert abc._class = "ABC"` both pass today). The
`_class` tag *is* the stored `∈ ABC` fact.

### `∩`/`∪` are universal set operators, not type-only syntax

Today `∩`/`∪` exist only inside type expressions. Under this model they
become general operators over *any* sets — value-sets, object-schemas,
numeric domains — uniformly:

- **`∩` = intersection = type extension / inheritance.** `C = A ∩ B` is the
  set of objects satisfying both A's and B's constraints — this *is*
  specialization. Requires open (structural) schemas — see Decision 3.
- **`∪` = union = union types.** `Status = {"Success"} ∪ { tag = "Error", code ∈ Number }`
  gives discriminated unions with no extra keyword.

## Literals

- **Value-set:** `{ 1, 2 }` — bare, listed elements. `[ ... ]` arrays stay
  separate (ordered, duplicates allowed — a resolved "values list", not a
  possibility space; unaffected by this ticket).
- **Object-schema:** `{ r ∈ a, k = 1, v ∈ Number }` — every entry is
  `name ∈ …` or `name = …`. The set of all objects satisfying these field
  constraints. Today's plain object literals (`{ name = "Ada" }`, all fields
  singletons) keep their exact current meaning — they're just the case where
  every field constraint happens to be `=`.
- **Nested sets:** `rt = { 1, 2, { k, lm } }` — an element that is itself a
  set. Works because sets are first-class values (Decision 1).
- **Correction (found during Phase 1 implementation):** mixing bare elements
  and `name = …` in one `{ }` is *not* a parse error, and doesn't need to be
  — there's no real ambiguity to guard against. `=` already exists as an
  expression-level equality operator (used e.g. inside `if x = y`), so
  `{ 1, name = "Ada" }` simply parses as a two-element set: the number `1`
  and the boolean result of `name = "Ada"` (a genuine equality-comparison
  expression, evaluated like any other set element — it only errors at
  runtime if `name` isn't a bound variable, same as any other expression
  would). Object-literal fields (`name = expr`) are a wholly separate
  grammar production, tried first and either fully consuming the `{ … }` or
  failing outright — there's no partial/ambiguous overlap with set-literal
  parsing to resolve.

## Decisions (all resolved 2026-08-05)

**Decision 1 — sets are first-class values.** Forced by nested sets
(`{1, 2, {k, lm}}`) and by `p = {1, 2}` needing to mean "p *is* the set"
(distinct from `p ∈ {1, 2}`, "p is 1 or 2"). New `Value::Set` variant.

**Decision 2 — `{ }` is empty object, not empty set.** No empty-set literal
is needed — `Null` already covers "no value here." `{}` reads as the schema
with zero field constraints, i.e. the set every object belongs to (vacuously
true predicate) — not a useless case, it's the universal object schema.
`{ a }` (single bare element) is a one-element value-set, not shorthand for
an object field.

**Decision 3 — object-schemas are open (structural).** `{x ∈ Number}` means
"objects with *at least* an `x ∈ Number` field" (at-least, not exactly). This
is required for `∩` to work as inheritance at all — a closed/exact reading
would make `A ∩ B` empty whenever A and B name different fields. A
constructed value (`ABC{...}`) is a concrete, closed instance that happens to
satisfy the open predicate; `∈`-membership tests stay structural.

## Relationship to shipped work

Several pieces already implemented this session are, in hindsight, this
model's primitives and stay as the mechanism (not legacy to preserve — they
*are* the set semantics, just not yet applied uniformly to every domain
kind): domain intersection with contradiction detection, singleton =
resolved, `=` routed through intersect, `Z/N/R` as integer/real domains,
domain-entailed comparisons (`assert b > 3` proven from a domain without
resolving it), and `loop` over a finite domain. What changes is the *surface*
(`{ }` set/object literals, universal `∩`/`∪`, sets-as-values, retracting the
false "∈ has two modes" framing) — the underlying intersect/contradiction
engine is exactly what gets reused and generalized.

## Implementation plan (agreed 2026-08-05)

Interpreter only — native codegen already rejects narrowing beyond
`=`/type-checks (T24) and stays rejecting through this ticket; revisit
native support separately, later, only if this gets real usage. Full
regression sweep (workspace build, `cargo test`, the `.code` suite, docs
examples, wasm smoke test, `fmt --check`) before and after every phase.

**Phase 0 — lock the design (this edit).** No code.

**Phase 1 — value-sets.** The first shippable slice:
1. `Value::Set(Vec<Rc<Value>>)` (`runtime.rs`) — dedup via the existing
   `values_equal`, `Display` (`{1, 2}`), `type_name` = `"Set"`.
2. `Expression::Set(Vec<Expression>)` (`ast.rs`).
3. Parser: `{` followed by a bare expression (not `name ∈/=`) parses as a
   value-set; `{}` parses as an empty object-schema (Decision 2). This is the
   riskiest grammar change — prototype and test it in isolation first.
4. Uniform `=`/`∈`: both route through `domain ∩= ⟦rhs⟧` with `=` wrapping a
   plain value in a singleton set first. No separate "type membership"
   branch — retract the old `MemberOf`/`IsType` split where it existed only
   to fake this.
5. `MemberOf` (today only accepts `Value::Array`) extended to accept
   `Value::Set` too.
6. `loop a { }` — enumerate a scalar's own finite domain in place (from
   T23): finiteness required (`ValueSet` or bounded `IntegerRange`; any
   `RealRange` or unbounded range rejected — preserves the "total work is
   statically bounded" guarantee); loop body is a block-scoped shadow; the
   outer variable is never resolved by the loop; `get` collection works like
   `loop x over y get result`.
7. `∩`/`∪` for value-sets (`{1,2} ∪ {3}`, `{1,2,3} ∩ {2,3,4}`).
8. Tests in `tests/` + new `docs/examples/*.code`, wired into existing CI.

**Phase 1: implemented (2026-08-05).** All 8 items above shipped. Notes on
what changed from the plan, and two real problems hit and fixed along the
way:

- The `{ }` disambiguation (item 3) turned out to need no lookahead trickery:
  `object_literal` is tried first and either fully matches or fails outright
  (object fields require `name = expr`, which a bare set element like `1`
  can never start with), so `.or(set_literal)` backtracks cleanly. The
  predicted "riskiest" part wasn't risky in practice.
- **Correction to item 3's "mixing is a parse error" framing** — see the
  note added to the Literals section above. Not a parse error; `=` already
  being a valid expression-level equality operator means a stray
  `name = expr` inside `{ }` just parses as a boolean set element, no
  ambiguity to guard against.
- **Real bug caught and fixed in the same pass:** `Domain::intersect()` had
  no arms for `ValueSet` against `Exact`/`ValueSet`/`RealRange`/
  `IntegerRange` — exactly the gap flagged back in T23. Concretely,
  `x ∈ {1,2}; x = 5` (5 not a member) silently succeeded instead of
  contradicting, because the mismatch fell into the generic `Intersection`
  wrapper, which never re-checks membership. Added the missing arms
  (`runtime.rs`) so set-narrowing actually intersects like every other
  domain kind — `x ∈ {1,2,3}; x > 1` now correctly narrows toward `{2,3}`
  and can fully resolve, and an out-of-set pin is now a real contradiction.
- **Real infra problem hit and fixed: adding `∩`/`∪` as two brand-new parser
  precedence tiers caused a stack overflow parsing *any* input, even
  trivial programs**, despite the CLI's existing 16MB parser thread
  (`main.rs`). This parser combinator (`chumsky`) encodes the whole grammar
  in the type system; the recursive `expr` in `build_expression` is already
  reused dozens of times (object fields, particle fields, conditions, loop
  bodies, ...), and two new wrapping tiers compounded far worse than
  linearly. Root-caused and fixed by folding `∩` into the existing
  `multiplicative` tier and `∪` into `additive` as extra operator
  alternatives (same precedence, zero new tiers) instead of adding new
  `.then().foldl()` layers — verified this removes the overflow entirely,
  same 16MB stack. A related, separately-triggered problem surfaced first
  during the same work: the *added* tiers (before being folded away) also
  blew a `rust-lld` debug-info relocation limit (`R_X86_64_32 out of
  range`) linking the final `code` binary (which statically embeds LLVM) —
  fixed regardless, and now a documented workspace default, by setting
  `[profile.dev] debug = "line-tables-only"` in the root `Cargo.toml` (no
  effect on program behavior, keeps backtraces working, just drops full
  per-type DWARF info that isn't needed for this project's debugging so
  far).
- Verified: full regression sweep green (workspace build, `cargo test`
  workspace, 152/152 `.code` fixtures — 8 new — `docs/examples/run.sh`, and
  the wasm smoke test against the real compiled `.wasm`, including two new
  smoke-test cases for the unresolved-domain and domain-entailed-assert
  bindings shape).

**Phase 2 — object-schemas: implemented (2026-08-05).** The design as
planned turned out to be simpler than T25 originally anticipated — see
below — and only `∩` (not `∪`) was in scope, matching the Decision-3
motivation.

**The key simplification: resolution stays per-*variable*, not per-*field*.**
T25 imagined `Value::Object` itself needing to represent a field with a
domain but no value (`HashMap<String, Either<Domain, Value>>`). That turned
out to be unnecessary. Instead:
- `Value::Object` is **completely unchanged** — still `HashMap<String,
  Rc<Value>>`, every field always resolved. Zero regression risk to any
  existing object code.
- New `Value::Schema(HashMap<String, Domain>)`: a resolved value in its own
  right — `K = { k ∈ String, m ∈ Number }` binds K to *this* schema (one
  well-defined value), even though the set of objects it describes is
  open-ended. Exactly parallel to `Value::Set` (Phase 1) — "resolved" just
  means the variable's *own* domain is a singleton, independent of whether
  what it holds is itself finite.
- New `Domain::Schema(HashMap<String, Domain>)`: narrows an *unresolved*
  scalar (`mm ∈ K`) to "must be an object satisfying K's fields" —
  structural, open (Decision 3: extra fields beyond what's named are fine).
- `mm = {...}` intersects `Domain::Schema` with `Domain::Exact(the object)`:
  checks every schema field against the object (open — missing named fields
  fail, extra unnamed ones don't), resolving on match, contradicting
  otherwise. `∩` on two schemas merges their field constraints (union of
  names, intersect where both define a field) — this **is** inheritance.
- This answers all four of T25's open questions for free, because none of
  them can arise: field access, spread, and host-boundary calls all require
  evaluating their receiver/argument first, which already fails with the
  ordinary "does not have a definite value yet" diagnostic for an
  unresolved (Schema-domain) variable — no new per-field logic needed.

**Grammar:** object-literal fields now accept `name ∈ expr` (or any
constraint operator) alongside `name = expr`, by routing the field's RHS
through the *same* `constraint_rhs` parser already used for top-level
`variable <op> expr` statements — a field constraint literally is a
constraint statement, just scoped to a field name. A literal with zero
constrained fields still produces a plain `Object` exactly as before
(`ObjectField::Static`, unchanged); one or more constrained fields makes it
a `Schema`. `T25`'s original gap (`{ k = 2, L ∈ Z }`) now parses and works.

**Two real bugs caught and fixed, surfaced by making schema-satisfaction
checking real:**
- `Domain::intersect()`'s `(Exact, TypeDomain)` arms passed through
  *unconditionally* for built-in type names — `m ∈ Number` never actually
  checked the pinned value was a `Number`. Invisible before Phase 2 (nothing
  exercised this path meaningfully); Phase 2's schema-satisfaction check
  depends on it being real, so it's fixed for every domain kind, not just
  schemas — `m ∈ Number; m = "x"` is now correctly a contradiction anywhere,
  not just inside a schema field. Custom `type X {...}` names are unchanged
  (still loose — checking those needs an Interpreter's type registry a pure
  `Domain::intersect()` doesn't have).
- `∈`'s Phase-1 grammar tries a bare capitalized identifier as a type name
  *before* falling back to a general expression — so `mm ∈ K` (K, the
  schema variable, capitalized in the worked example) parsed as
  `IsType(Named("K"))`, never reaching the new Schema-membership logic at
  all, and silently meant "mm._class is 'K'" (never true) instead of "mm
  satisfies K". Fixed at the `IsType` conversion step: if the name resolves
  to a bound variable holding a `Set` or `Schema` value, narrow against it
  structurally, the same as `MemberOf` would; only falls back to ordinary
  `_class`-based type-domain checking otherwise. A declared `type K {...}`
  can't collide with this — a name can't be both a bound variable and a
  declared type simultaneously.

**Known, deliberately deferred limitation:** `KK = String` (aliasing a
built-in type name to a variable, then using `k ∈ KK`) does not work — bare
`String`/`Number`/etc. aren't expressions today (`String` alone evaluates as
an undefined-variable lookup). The owner's worked example used this as a
minor illustrative aside, not the core mechanism; writing the schema with
the literal type name directly (`k ∈ String`) works fully. Making built-in
type names into genuine first-class values is a separate, smaller follow-up
if it's ever needed.

Native codegen rejects object-schemas cleanly (particle construction and
plain object literals both), consistent with T24 — same "no possibility of
silent divergence" bar as Phase 1.

Also fixed in passing: the code-wasm smoke test asserted an exact
`bindings` array order, which is backed by a `HashMap` and was never
actually guaranteed — confirmed by two consecutive wasm rebuilds (Phase 1
vs. Phase 2, no change to the relevant code) producing different but
internally-consistent orderings. Comparison is now order-independent
(sorted by name) rather than chasing the ordering itself.

6 new `tests/*.code` fixtures (basic schema resolution + the T25 mixed-field
case, the refuted-field case, missing-required-field, `∩`-inheritance
success/failure, unresolved-schema field access). Full regression sweep
green: workspace build, `cargo test`, 158/158 `.code` fixtures (6 new),
`docs/examples/run.sh`, wasm smoke test against the real compiled `.wasm`,
`fmt --check`.

**Phase 3a — discriminated unions: implemented (2026-08-05).** Split from
flow-sensitive narrowing (3b, below) — large enough on its own, and
narrowing needs its own design pass.

- New `Value::Union(Vec<Domain>)` / `Domain::Union(Vec<Domain>)`, exactly
  parallel to Phase 1's `Set` and Phase 2's `Schema` — `Status = {"Success"}
  ∪ {tag = "Error", code ∈ Number}` binds Status to *this* union, one
  well-defined resolved value, even though what it describes is "one of
  several alternative shapes."
- `∪` generalized: `Set ∪ Set` keeps its Phase 1 fast path (flat merged
  Set — no discrimination machinery needed when both sides already
  enumerate the same way). Anything involving a `Schema` (which can't
  collapse to a flat Set — open/predicate membership) or an existing
  `Union` (flattened in, never nested) produces a `Value::Union` instead.
- `s ∈ Status` narrows the scalar `s` to `Domain::Union` (via the same
  `MemberOf` path Set/Schema already use, plus the same bound-uppercase-
  variable fallback in the `IsType` conversion that Phase 2 needed for
  `mm ∈ K`). `s = v` resolves if `v` satisfies *any* alternative
  (`intersect()`'s `(Union, Exact)` arm — non-empty against at least one
  member) and contradicts only if it matches none — reusing whatever
  intersection logic each alternative already has (a Schema member still
  enforces its own field constraints; verified `code ∈ Number` still
  rejects a String even inside the union's Error branch).
- **Discrimination needs no new mechanism.** Once `s` resolves, asking
  which alternative it took is just an ordinary membership/type check
  (`if s ∈ Object { ... }` vs. `if s ∈ String { ... }`) — verified directly,
  works with zero Union-specific code.
- Native codegen already rejected `∪` entirely since Phase 1
  (`BinaryOp::Intersect | BinaryOp::Union => Err(...)`, blanket, not
  operand-specific) — so heterogeneous unions are already cleanly rejected
  with no additional codegen work.
- 3 new `tests/*.code` fixtures (union creation + both branches resolving +
  discrimination; refuted-neither-branch; refuted-matches-shape-but-fails-
  a-field-constraint). Full regression sweep green: workspace build,
  `cargo test`, 161/161 `.code` fixtures (3 new), `docs/examples/run.sh`,
  wasm smoke test against the real compiled `.wasm`, `fmt --check`.
- Not attempted / explicitly out of scope for 3a: `Union ∩ Union`,
  `ValueSet`/`Schema` combined with an existing `Union` via `∩` — these
  fall into the pre-existing generic `Intersection` wrapper (stuck
  unresolved rather than wrong) rather than a dedicated arm. No motivating
  example needed one; add if one ever does.

**Phase 3b — flow-sensitive narrowing: implemented (2026-08-05).**

**Scope decision (owner, 2026-08-05):** block-scoped only, no cross-
statement memory. `if s ∈ Object { ... }` narrows `s` *inside that block
only*; a later, separate `if s ∈ String { ... }` doesn't know the first
branch ruled out Object — it re-decides from `s`'s original (unnarrowed)
domain independently. Full TypeScript-style flow analysis (narrowing that
persists across sequential `if`/`if not` pairs) was explicitly rejected as
a much larger, separate mechanism not worth building without a concrete
need for it.

**Mechanism:** `Statement::If` intercepts the specific condition shape
`<var> ∈/∉ TypeName` before falling through to ordinary evaluation (mirrors
how `assert`'s domain-entailed comparisons already intercept `Binary`
conditions in Phase-1-era work). For an *unresolved* `<var>`, decides from
its domain via a new `split_domain_by_type_name` (runtime.rs): `AlwaysTrue`
runs the block unchanged, `AlwaysFalse` skips it, and the mixed case runs
the block with `<var>` **shadowed** to the narrowed domain via
`env.define_with_domain` inside a pushed scope — exactly `loop <var> { }`'s
existing block-scope-shadow pattern (Phase 1), just triggered by `if`
instead of `loop`. A resolved `<var>` (or any other condition shape) falls
straight through to the pre-existing evaluation path, untouched.

**Splitting a domain by type name** (`partition_domain_member`) handles the
cases that actually arise from Sets/Schemas/Unions: a `ValueSet` partitions
its elements by actual value type; a `Schema` is wholly Object-shaped or
wholly not; ranges are wholly Number-shaped or not; a `Union`'s members are
each partitioned and recombined. Anything without a clear split (a custom
`type X {...}` name, `Any`, `Intersection`) is conservatively treated as
fully matching — narrowing is skipped there rather than risking an
incorrect split; no motivating example needed tighter handling.

**A pleasant emergent behavior, not a special case:** when narrowing
happens to collapse to exactly one possible value (e.g. `if s ∉ Object`
narrows a two-branch union down to a one-element `ValueSet`), the shadowed
`s` is *already* a resolved singleton the instant the block is entered —
`is_singleton()`'s existing one-element-`ValueSet` rule handles this for
free. Verified directly: inside such a block, `s` is usable immediately, no
`=` needed, and *writing* `s = "Success"` there correctly hits the ordinary
single-assignment-reassignment error (same as `a = 1; a = 1` anywhere else)
rather than needing special-casing.

**Verified with the full matrix:** mixed-domain narrowing lets a matching
value resolve inside the block; a value from the *excluded* alternative
still contradicts (proves real narrowing, not just a label — `fail_flow_
narrowing_excluded_branch.code`); negation (`∉`) narrows to the
complementary alternative; already-decided conditions (`AlwaysTrue`/
`AlwaysFalse`) skip narrowing entirely; the outer variable is provably
untouched after the block (resolving it to the *other*, would-be-excluded
alternative afterward still works, since nothing leaked).

No codegen changes needed — a program reaching this pattern already fails
to compile earlier, at the `∪`/Schema-establishing statement (T24/3a's
existing blanket rejection), before the native backend would ever see the
`if`. 2 new tests. Full regression sweep green: workspace build, `cargo
test`, 163/163 `.code` fixtures (2 new), `docs/examples/run.sh`, wasm smoke
test against the real compiled `.wasm`, `fmt --check`.

This closes T26's phased plan (1, 2, 3a, 3b) — every decision the owner and
I worked through across this whole design conversation is now implemented
and verified, from the original "a variable is always a set of possible
values" thesis through discriminated unions and flow narrowing.

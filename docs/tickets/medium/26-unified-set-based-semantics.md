# 26 — Unified set-based semantics: variables are constraint sets, types are sets, `=`/`∈`

- **Priority:** Medium (foundational / large — a language-model redesign, not a feature)
- **Type:** Language design / core semantics
- **Supersedes:** [T23](23-set-domain-and-possibility-enumeration.md) (set literals + `loop`), [T25](25-partially-resolved-objects.md) (objects with unresolved fields) — both fold into this unified model.
- **Status:** Design in progress. Three core decisions are still open (marked **DECISION** below). Not implementation-ready until they're resolved.

## The model (settled over several design conversations, 2026-08)

The owner's thesis, developed and stress-tested across many rounds: **there is
no "type" vs "value" distinction. A variable always holds a *set of possible
values* — its domain. Sometimes that set is a singleton, and then it reads as
an ordinary value.** Everything else falls out of this.

- `a ∈ {1}` reduces to `a = 1` — a singleton domain *is* assignment.
- `x = v`  means `domain(x) = {v}`   (equality — the domain is exactly this).
- `x ∈ S`  means `domain(x) ⊆ S`     (subset — narrow to a subset of S; further
  constraints can narrow it more).
- Resolution = the domain has collapsed to a singleton. Using an unresolved
  variable where a single value is required is an error; using it where a set
  is acceptable (e.g. `loop`) is fine (this is already implemented — see the
  domain-entailment and `loop`-over-domain work).

`=` is just the special case of `∈` whose right side is a singleton. One
mechanism, two notations.

### Language `∈` = domain `⊆`

`c ∈ a` ("c is an element of set a") means exactly `domain(c) ⊆ a`. So the
language never needs a separate subset (`⊆`) operator — Gemini's "`B ⊂ A` not
`B ∈ A`" caveat is handled at the language level by `∈` already. Membership of
a *scalar in a set* and *narrowing a domain by a set* are the same operation.

### Constraints on the same variable intersect

`e ∈ X; e ∈ Y` ⟹ `domain(e) = X ∩ Y`. Always. There is no separate "add a
constraint" rule; every constraint on a variable is intersected into its
domain (this is exactly what the interpreter's `apply_constraint` already
does). This answers "if I later write `e ∈ {r = 1}`, is it an intersection?" —
yes, unconditionally.

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

### Set operations are universal (not type-level only)

Today `∩`/`∪` exist only inside type expressions (`Number ∪ String`). Under
this model they become general operators over *any* sets — value-sets,
object-schemas, numeric domains — uniformly:

- **`∩` = intersection = type extension / inheritance.** `C = A ∩ B` is the
  set of objects satisfying both A's and B's constraints. Adding constraints
  narrows the set — that *is* specialization. Predefined props of A and B
  both apply to C (union of predefined props).
- **`∪` = union = union types.** `Status = {"Success"} ∪ { tag = "Error", code ∈ Number }`
  gives TypeScript-style discriminated unions with no extra keyword. A value
  `s ∈ Status` is discriminated by membership tests (`if s ∈ {"Success"}` vs
  the object branch) — heterogeneous unions require this.

## Literals

- **Value-set (extensional):** `{ 1, 2 }` — listed elements. `[ ... ]` arrays
  stay separate (ordered, duplicates allowed, a resolved "values list"); `{ }`
  sets are unordered/deduplicated possibility spaces.
- **Object-schema (intensional):** `{ r ∈ a, k = 1, v ∈ Number }` — the set of
  all objects satisfying these field constraints. A concrete object literal
  (`{ name = "Ada" }`, all fields singletons) is just the singleton case of
  this, so today's object literals keep their meaning unchanged.
- **Nested sets:** `rt = { 1, 2, { k, lm } }` — a set with a set as an
  element. Requires sets to be first-class *values* (see DECISION 1).

## Open decisions (must be resolved before implementation)

### DECISION 1 — Are sets first-class *values*, or only domains?
`rt = { 1, 2, { k, lm } }` (an element that is itself a set) and the need for
`a = {1,2}` (a *is* the set) to differ from `a ∈ {1,2}` (a is 1 or 2) both
**force sets to be values**, not just domains. This reverses the tentative
"sets are only domains" lean from T23's discussion.
**Recommendation: yes — sets are first-class values** (a new `Value::Set`),
because the owner's own examples require it. `domain(a)` for `a = {1,2}` is the
singleton `{ the-set-{1,2} }`; the "variable is a set of values" purity holds
(the single value it holds just happens to be a set).

### DECISION 2 — `{ }` disambiguation rule: set vs object.
Entries are all bare expressions → value-set; entries are all `name ∈/= …` →
object-schema. **Mixing the two in one literal is a parse error** — this is
exactly what makes the malformed `{"Error", code ∈ Number}` illegal and forces
discriminated unions to use a tag field. Edge cases needing a firm call:
- `{ a }` — bare element (set containing a's value) or shorthand object with
  field `a`? **Recommendation: bare element (value-set).** Object fields
  always need an explicit `∈`/`=`.
- `{ }` — empty set or empty object? **Recommendation: empty value-set (`∅`);**
  the "set of all objects" (empty object-schema) is rare and can be spelled
  explicitly if ever needed.

### DECISION 3 — Object-schemas: open (structural) or closed (exact)?
`∩ = inheritance` **only works if schemas are open** — `{x ∈ Number}` must mean
"objects with *at least* x:Number", or else `A ∩ B` is empty and inheritance
collapses.
**Recommendation: schemas are open as membership predicates (at-least these
fields); a constructed value (`ABC{...}`) is a closed, specific instance that
satisfies the open predicate.** This makes `∩` inheritance work, keeps
`abc ∈ ABC` as a structural match, and matches how structural typing +
construction normally coexist.

## Relationship to shipped work

Several pieces already implemented this session are, in hindsight, this model's
primitives and should be kept as the mechanism (not "legacy to preserve" — they
*are* the set semantics): domain intersection with contradiction detection,
singleton = resolved, `=` routed through intersect, `Z/N/R` as integer/real
domains, domain-entailed comparisons (`assert b > 3` from a domain), and
`loop` over a finite domain. What changes is the *surface* (`{ }` set/object
literals, universal `∩`/`∪`, sets-as-values) and the framing (types = sets).

## Scope / non-goals for a first cut

- Interpreter first; native codegen already rejects narrowing beyond
  `=`/type-checks (T24), so it stays rejecting until/unless a later phase.
- This is large enough to phase: (1) value-sets + `=`/`∈` + `loop`; (2)
  object-schemas open/closed + `∩`/`∪` as universal ops; (3) unions +
  discrimination. Each phase is independently testable.

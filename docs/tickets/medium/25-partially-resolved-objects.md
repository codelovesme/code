# 25 — Objects with unresolved (constrained-only) fields

- **Priority:** Medium
- **Type:** Language design / core semantics
- **Area:** `src/runtime.rs` (`Value::Object` representation), `src/parser.rs` (object-literal grammar), `src/interpreter.rs` (field access, construction)
- **Depends on:** [T23](23-set-domain-and-possibility-enumeration.md) (same "a variable can be constrained but not resolved" model, applied one level deeper)

## The gap

Verified directly: object literals cannot mix a resolved field with a
constrained-only one.

```
a = { k = 2, L ∈ Z }
```

→ `error: Unexpected: a = { k = 2, L ∈ Z }`

Object-literal fields only ever come in one shape —
`ObjectField::Static(name, value)` / `ObjectField::Computed(key, value)`
(`parser.rs:128,139`), i.e. `name = expr`. There's no field form that means
"this field is only type/domain-constrained, not yet a concrete value."
`Value::Object` itself is `HashMap<String, Rc<Value>>` — every entry must
already be a fully resolved `Value`; there's no way to store "a field with a
domain but no value" at all today.

## What's wanted (owner's example, 2026-08-05)

You have a variable known to be shaped like an object, where one property is
known to be an integer (constrained, not yet pinned to a specific number)
and another is already known to be `2` — and this should be representable
and storable, the same way T23 lets `A = ⦃1, 2⦄` be a known set and
`b ∈ A` say `b` is one of its elements without either being resolved to one
concrete number.

## Why this is a separate ticket from T23, not folded into it

T23's mechanism is scoped to *scalar* variables (a name → one `Domain`).
This is a different, deeper mechanism: a *field inside a structured value*
carrying a `Domain` instead of a `Value`. That means `Value::Object` itself
needs to represent partial resolution — e.g.
`HashMap<String, Either<Domain, Rc<Value>>>` or equivalent — which touches
everything that reads an object's fields, not just constraint statements:

- **Field access** (`obj.L`) — must behave like referencing an unresolved
  scalar (the "does not have a definite value yet — …" diagnostic from the
  `=`-pin fix) rather than a plain `Value` lookup.
- **Structural type-checking** (`∈ SomeType`, used everywhere in real
  handler signatures) — does a type check pass against an object with an
  unresolved field, or does it require full resolution first?
- **Spread/construction** (`NormalizedTask { ...raw_t }`, seen throughout
  the real corpus) — how does spreading into a typed record interact with a
  source field that isn't resolved?
- **Serialization at host boundaries** (`server.Respond { body = … }`,
  `json.Parse`/emit) — an object with an unresolved field almost certainly
  *cannot* cross a host boundary (there's no way to JSON-encode "an
  integer, TBD") — needs an explicit rule (reject at the boundary? require
  full resolution before any native/host handler call?).

None of this exists in T23's scope (bare scalars only) — worth its own
design pass once T23's simpler case has shipped and the `Domain`-borrowing
plumbing (`env.get_domain`, `apply_constraint`, `intersect`) has a working,
tested precedent to build on.

## Not started

This ticket records the requirement and the reason it's scoped separately;
no design decisions beyond the above have been made yet (representation
choice, field-access semantics, and the host-boundary rule are all open).

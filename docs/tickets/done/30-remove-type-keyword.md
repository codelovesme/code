# 30 — Remove `type`: particle types are plain `∩`-merged Schema variables

Status: Implemented and shipped (2026-08-06).

## The decision

`type X { field ∈ Type, ... }` did two unrelated jobs: (1) name a schema,
and (2) stamp constructed instances with a `_class`/`_created` tag. T26
already concluded (`26-unified-set-based-semantics.md`, "Types are sets;
`type` is (mostly) sugar") that job (1) is fully redundant with a plain
object-schema literal (`X = { field ∈ Type }`) and existing `∩`/`∪` —
`type`'s only non-redundant contribution was job (2) — and flagged full
removal as "deferred, not rejected."

Removed now, with no implicit magic: a predefined `Particle` schema
supplies `_class`/`_created`, and the declarer pins `_class` explicitly:

```
Particle = { _class ∈ String, _created ∈ Number }
Log = Particle ∩ { _class ∈ "Log", message ∈ String, level ∈ "Error" ∪ "Info" }
```

`∩` between two `Value::Schema`s already merged this way (T26 Phase 2,
`merge_schemas`) — no new merge logic was needed for the mechanism
itself. The `type` keyword, `Statement::TypeDeclaration`, the combined
`ClassName{field ∈ Type} => {}` handler sugar, and `Environment`'s
separate `type_registry` are all gone; only the bare `ClassName => {}`
handler form remains.

**Native codegen explicitly does not get an equivalent** (user decision):
it loses type-based construction validation, spread-field enumeration,
and native-emission-drain field rebinding for particle types declared
the new way, since that would require compile-time-evaluating `∩`,
which codegen doesn't do. This matches the standing T26+ pattern —
`∩`/`∪` are already codegen-rejected — so an in-source particle type
declared via `Particle ∩ {...}` was never going to be natively
compilable regardless. Untyped/unvalidated particle construction and
**native module linking** (importing a compiled `.so`/`.wasm` with
ABI-declared types) are unaffected — the ABI's `FieldConstraint`/
`TypeInfo` bridge doesn't go through the `type` keyword at all.

## What changed

- **Parser**: `type_field`/`type_decl` and `handler_type_field`/
  `combined_handler_def` deleted; `type` dropped from the reserved-
  identifier list.
- **AST**: `Statement::TypeDeclaration` deleted; `HandlerDefinition`
  loses `inline_type`. `FieldConstraint`/`TypeInfo` are **kept** — still
  fed by the native/wasm ABI bridge (`native_module.rs`, `wasm_module.rs`),
  entirely independent of the `type` keyword.
- **Environment**: `type_registry` and `define_type`/`get_type` deleted.
  `save_and_isolate_scopes`/`restore_scopes` deleted too — confirmed
  zero call sites anywhere in the repo, already dead code that
  referenced `type_registry`'s shape.
- **Interpreter**: `Interpreter::new()` now parses and executes a small
  bootstrap source (`Particle = {...}`; `Exception = Particle ∩ {...}`)
  through the normal statement path, reusing existing evaluation
  machinery instead of hand-building a `HashMap<String, Domain>` in
  Rust. `Expression::Particle` construction's validation source moved
  from `env.get_type(type_key)` to `env.get(type_key)` + a
  `Value::Schema` check, with a new `domain_permits_missing()` helper
  for optional-field detection (deliberately *not* a generic
  intersect-with-`Null` probe — see the correctness fixes below for
  why that's unsound). The four `Statement::Import`/`NativeImport` call
  sites that used to call `define_type` for qualified module types now
  call `env.define(qualified_name, Value::schema(...))` via a new
  `field_constraints_to_schema()` bridge — the one place genuinely new
  conversion code was needed, since native/wasm-ABI-derived types never
  flow through the ordinary variable-export path.
- **A gap found and fixed during testing**: unqualified module exports
  of a Schema variable (`link mod` with no alias) worked immediately
  with zero changes — `Value::Schema` flows through the ordinary
  variable-export path like any other value. But *qualified* particle
  construction (`m.Log { ... }`) did not — nothing bound the synthetic
  dotted key `"m.Log"` for a plain `.code`-module export (only
  native/wasm-ABI types got that treatment). Fixed by also binding
  `alias.name` for every `Value::Schema` in an aliased module's export
  map, closing what was otherwise a silent validation regression
  (`m.Log { totally_wrong_field = 1 }` constructed successfully,
  unvalidated, before this fix).
- **Codegen**: `Statement::TypeDeclaration`/`inline_type` compile arms
  deleted. Everything else (`get_type_def`, codegen's own parallel
  type registry, `compile_import`/`compile_native_import`) is untouched
  — still fed by native/wasm-imported types, a genuinely separate,
  still-functioning mechanism.
- **LSP**: `type` dropped from syntax-highlighting keywords and
  completion suggestions; the (partly already-dead — its `type Name =
  ...` "alias" half matched no real grammar) document-symbol scanner
  block removed; a `particle` snippet added in place of the old `type`/
  `type alias`/combined-handler snippets.
- **Migration**: all 34 `.code` files across `tests/` and `docs/examples/`
  using `type X {...}` or the combined handler syntax rewritten to
  `X = Particle ∩ { _class ∈ "X", ... }` + a bare handler. `site/guide.html`
  §04 and `docs/examples/04-types.code` reworded around `Particle`/`∩`
  instead of the `type` keyword.

## Correctness bugs found and fixed along the way

Migration surfaced three real, pre-existing bugs that this design leaned
on hard enough to expose:

1. **`Domain::intersect`'s `(Exact, TypeDomain)` arms only checked a bare
   `TypeExpr::Named` precisely** — `Literal`/`Union`/`Intersection` shapes
   silently passed through unconditionally. This meant `_class ∈ "Log"`
   and `level ∈ "Error" ∪ "Info"` — exactly the shapes this whole
   redesign depends on — never actually rejected a mismatch (`Log {
   level = "TOTALLY_INVALID" }` constructed without error). Fixed with a
   new `value_matches_type_expr_shallow()` that handles every `TypeExpr`
   shape except a non-builtin `Named` (which still needs interpreter env
   access, unavailable to this free function — unchanged, pre-existing
   limitation).
2. **`(TypeDomain, TypeDomain)` intersection had no dedicated arm** — it
   fell through to the generic `Domain::Intersection` wrap, which the
   fix above couldn't see through (the top-level shape was `Intersection`,
   not `TypeDomain`, by the time a later `Exact` check ran). This is
   exactly what `merge_schemas` exercises whenever `Particle`'s
   `_class ∈ String` merges with a declarer's `_class ∈ "Log"`. Fixed by
   folding `(TypeDomain(a), TypeDomain(b))` into
   `TypeDomain(TypeExpr::Intersection([a, b]))` directly, keeping the
   merged constraint inside the one shape that's actually checked
   precisely.
3. **`object_satisfies_schema` treated a missing field as an unconditional
   failure**, with no allowance for a field whose domain would accept
   `Null`. This broke `∈ Exception` for the interpreter's own internally
   built "assertion failed" exception object (missing `_created`, and
   in one case genuinely missing it — a separate omission also fixed).
   Fixed by treating an absent field as `Value::Null` for the
   containment check — a required field's domain never accepts `Null`
   anyway, so this only changes the outcome for fields that were
   actually optional.

Also found: `Interpreter::new()`'s bootstrap bindings (`Particle`,
`Exception`) were leaking into `Environment::bindings()`/
`bindings_detailed()` — the exact mechanism a host UI (the playground,
T19) uses to render "what did this program produce." Every program's
result panel would have silently gained two extra, confusing built-in
entries. Fixed with a `builtin_names` exclusion set on `Environment`,
populated once by the bootstrap.

## Verification

Full regression sweep — workspace build, `cargo test --workspace`, the
`.code` suite (169/169), `docs/examples/run.sh`, wasm rebuild + smoke
test (including a rebuild after the bootstrap-leak fix, since the first
attempt caught the wasm smoke test's own resolved-bindings assertion
failing), `code fmt --check`, a manual native `code build` smoke test
confirming untyped particle construction/dispatch still round-trips
correctly — all green. `tests/euglena/`'s own custom "Exception"-shaped
logging particle (source/message, unrelated to the language's built-in
Exception) was renamed to `LogException` to avoid colliding with the
now-reserved `Exception` binding.

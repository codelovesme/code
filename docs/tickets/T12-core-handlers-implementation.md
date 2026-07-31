# T12 — Implement `to core` handler dispatch; remove `Expression::Call`

- **Priority:** High
- **Type:** Implementation (follow-up to T11's decision: Option A, full retirement)
- **Area:** `ast.rs`, `parser.rs`, `interpreter.rs`, `codegen.rs`, `runtime_native.c`,
  `wasm_module.rs`

## Scope

Retire `Expression::Call` (the `name(args)` call-expression node) entirely and
re-express `timestamp`/`length` as handlers dispatched via a new reserved
target: `emit X { ... } to core get result`.

```
emit Length { value = arr } to core get n
assert n = 3

emit Timestamp {} to core get t
assert t ∈ Number
```

No `type Length {}` / `type Timestamp {}` declaration is required — like
native-module handlers, these dispatch by `_class` name and construct/return
dynamic particles (see README: "If no local type is in scope... particle
construction falls back to dynamic behavior").

## 1. Remove `Expression::Call`

It has no purpose beyond the two hardcoded names (any other callee/name is
already rejected — `"Only named function calls are supported"` /
`"Unknown function: {name}"`), so remove it outright rather than leaving dead
grammar:

- `ast.rs`: delete the `Call { callee, args }` variant from `Expression`.
- `parser.rs`: remove the grammar production that parses `identifier(args)` as
  a call expression (postfix chain alongside `IndexAccess`/`PropertyAccess`).
- `interpreter.rs`: remove the `Expression::Call` match arm in `eval_expr`
  (currently `src/interpreter.rs:1202-1227`).
- `codegen.rs`: remove the `Expression::Call` match arm and the `compile_call`
  function entirely (currently `src/codegen.rs:4085`, the LLVM reimplementation
  of `timestamp`/`length`).

## 2. Add `HandlerTarget::Core`

- `ast.rs`: add `Core` to `HandlerTarget` (alongside `This`, `Base`,
  `ModuleAlias`).
- `parser.rs`: extend the `handler_target` parser
  (`src/parser.rs:811`) with `.or(text::keyword("core").to(HandlerTarget::Core))`.
  `core` is not currently a reserved word — no collision.

## 3. Interpreter dispatch

Add a `dispatch_core_handler(&self, class_name: &str, particle: &Value) ->
Result<Rc<Value>, String>` with a plain match on `class_name`
(`"Timestamp"`, `"Length"`, ...). This does **not** need the
`NativeHandlerInfo`/`NativeFnPtr` closure-registry machinery used for loaded
`.so` modules — that indirection exists to hold function pointers resolved at
`dlopen` time from a variable set of *loaded* libraries. Core handlers are a
small, fixed, compiled-in set — a direct match arm is simpler and doesn't
pretend they're user-extensible.

Wire it into the existing handler-target dispatch alongside `This`/`Base`
(`src/interpreter.rs`, near `HandlerTarget::Base => ...`).

## 4. Codegen dispatch (LLVM path)

This is **not** a natural fit for the native-module dispatch path used for
`.so`/`.wasm` handlers — that path is `dlopen` + runtime pointer resolution,
which a compiled `.exe` needs at *program* runtime, not something available at
LLVM codegen time for a fixed compiled-in set. Instead, mirror how
`__value_to_cstr` already works (`src/runtime_native.c:464`):

- Add `__core_timestamp(void) -> CodeValue` and
  `__core_length(CodeValue) -> CodeValue` to `runtime_native.c`.
- Since T8, this C bridge is **always** compiled and linked into every native
  `exe`/`shared`/`static` build (not conditionally, as it was before) — so no
  new linking logic is needed, only new functions in the existing file and new
  LLVM `declare`/`call` sites in codegen, replacing `compile_call`'s inline IR
  for `timestamp`/`length`.
- `to core` dispatch in codegen compiles a direct call to the matching
  `__core_*` C symbol, the same way other bridge calls already do.

## 5. `.wasm` ABI cleanup

Once no built-in relies on a function-export concept, drop the dead
`fns_ptr`/`fn_count` slot reservation in the `.wasm` module descriptor
(`src/wasm_module.rs:22-23,36,142,152` — currently parsed-but-unused,
`"skipped — host does not call exported fns"`). `.wasm` modules built against
the old layout would need their descriptor size/offsets bumped accordingly —
confirm no shipped `.wasm` fixtures depend on the old byte layout before
removing (check `tests/native_modules/test_math_wasm.c` and rebuild).

## 6. Tests

- `.code` fixtures: `emit Length{value=[1,2,3]} to core get n; assert n = 3`
  and a `Timestamp` smoke test, run under **both** `code run` (interpreter) and
  `code build --target exe` (codegen) — mirroring how other dual-backend
  features are tested in this suite (see `tests/llvm_codegen.rs`).
- Negative test: calling `to core` with an unknown class name errors clearly
  (equivalent of today's `Unknown function: {name}`).
- Confirm `code fmt` and the `abi_header_sync` drift guard (T2) are unaffected
  (no ABI struct changes here, only new C *functions*, not new struct fields).

## Acceptance criteria

- `grep -rn 'Expression::Call' src/` returns nothing.
- `timestamp`/`length` work identically to today via `to core`, under both the
  interpreter and the LLVM backend (`--target exe`).
- Full suite (`cargo test --workspace`, `code test`, `code fmt . --check`)
  green.

## Effort

Medium. Touches five files but each change mirrors an existing, working
pattern in the same file (no new architecture invented) — closer to plumbing
than design.

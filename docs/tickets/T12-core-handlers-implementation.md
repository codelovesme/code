# T12 [DONE] — Implement `to core` handler dispatch; remove `Expression::Call`

- **Priority:** High
- **Type:** Implementation (follow-up to T11's decision: Option A, full retirement)
- **Area:** `ast.rs`, `parser.rs`, `interpreter.rs`, `codegen.rs` (planned to also
  touch `runtime_native.c`/`wasm_module.rs`; see Resolution — neither ended up
  needed/done)

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

Planned: Medium, five files. Actual: four files (`runtime_native.c` turned out
unnecessary — see Resolution) — each change mirrored an existing, working
pattern in the same file, closer to plumbing than design.

## Resolution (implemented)

Items 1, 2, 3, and 6 landed exactly as planned. **Item 4's plan was corrected
during implementation** (no C bridge functions needed after all — see below).
**Item 5 (`.wasm` dead ABI slot) was not done** — deferred as a separable
follow-up, not required for `to core` to work.

- **1. `Expression::Call` removed outright**: the `Call` variant is gone from
  `ast.rs`, its postfix `(args)` grammar production is gone from `parser.rs`
  (parenthesized *grouping* — `(1 + 2)` — is a separate, untouched rule), and
  both match arms (`interpreter.rs`, `codegen.rs`) are deleted along with the
  now-dead `compile_call`. `grep -rn 'Expression::Call' src/` is empty.
- **2. `HandlerTarget::Core`** added (`ast.rs`) and parses via
  `text::keyword("core")` alongside `this`/`base` (`core` wasn't reserved — no
  grammar conflict).
- **3. Interpreter**: a free function `dispatch_core_handler(class_name, particle)`
  (not a method — no `self` needed) handles `"Timestamp"`/`"Length"` with a
  plain match, called from an early-return special case in
  `exec_handler_invoke` before the body/native dispatch logic runs. The two
  now-non-exhaustive `match target` sites use `unreachable!()` for `Core`
  (genuinely unreachable given the early return) — the same idiom already used
  elsewhere in this codebase.
- **4. Codegen — plan corrected during implementation**: the plan above assumed
  `compile_call`'s `timestamp`/`length` called into `runtime_native.c` (like
  `__value_to_cstr` does), so it proposed adding `__core_timestamp`/
  `__core_length` C functions there. Reading `compile_call` showed this was
  wrong — it already built the result **entirely as inline LLVM IR** (calls to
  already-declared `time_fn`/`strlen_fn`, no bridge involved). So instead: that
  exact IR was moved into a new `compile_core_handler` method (called from
  `compile_handler_invoke`'s new `Core` early-return, mirroring the
  interpreter), and a new `build_core_result` helper wraps its output as
  `{ _class = "<X>Result", value = ... }` — a two-field object built with the
  same malloc+`field_type`-array pattern `compile_object_fields` already uses.
  **No `runtime_native.c` changes were needed** — lower risk than the original
  plan, one fewer file touched.
- **5. `.wasm` dead ABI slot — not done, deferred**: unaffected by item 4's
  correction and separable from everything else here; left as a distinct,
  low-risk future cleanup.
- **6. Tests**: `tests/core_handler_length.code`, `tests/core_handler_timestamp.code`,
  `tests/fail_core_handler_unknown.code` (interpreter path, run via `code test`);
  `build_core_handler_length_exe_runs` in `tests/llvm_codegen.rs` (compiled
  `--target exe` path). Verified interpreter/codegen parity directly: `Length`
  on an array, a string, and an empty array; `Timestamp`; unknown class error.
- **Full suite green**: `cargo test --workspace` (all green, `llvm_codegen`
  18/18), `.code` suite 137/137 (+3), `code fmt . --check` canonical (155
  files), `code-lsp` still has no `inkwell`/`llvm-sys` in its dependency tree,
  `abi_header_sync` (T2's drift guard) unaffected.

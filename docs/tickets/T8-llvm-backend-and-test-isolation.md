# T8 — LLVM backend `+` link bug, default-target disagreement, and test isolation

- **Priority:** High (first item is a real backend bug)
- **Type:** Bug / test infrastructure
- **Area:** `src/codegen.rs`, `src/main.rs`, `src/linker.rs`, `tests/llvm_codegen.rs`
- **Origin:** uncovered while greening the suite in T6.
- **Status:** DONE — `cargo test --workspace` is fully green and order-independent.

## Resolution (implemented)

1. **`+` link bug:** the C bridge (`runtime_native.c`, which defines
   `__value_to_cstr`) is now compiled and linked for every native `exe`/`shared`
   build, not only when native modules are imported (`src/main.rs`). The bridge
   is compiled with `-fPIC` so it links into shared libraries too. Verified `+`
   now builds and runs across `exe`/`shared`/`static`/`wasm`.
2. **Default target:** aligned to **`exe`** (owner decision). README table
   updated; `build_default_is_ir` rewritten as `build_default_is_exe`.
3. **Test isolation:** intermediate object and C-bridge filenames are now tagged
   per-module *and* per-target (`<name>.<kind>.o`,
   `__code_runtime_native_<name>_<kind>.o`), removing the shared-file race.
   `cargo test --test llvm_codegen` is now 17/17, deterministic across parallel
   runs.
4. **(bonus) Broken doctests:** `src/wasm_module.rs` had nine bare ```` ``` ````
   fences around ASCII memory-layout tables that rustdoc tried to compile as
   Rust (masked earlier because the suite was already red). Annotated as
   ```` ```text ````.

## 1. `+` on non-native programs fails to link (`__value_to_cstr`)

The polymorphic `+` operator always emits a reference to the runtime symbol
`__value_to_cstr` (the string-concat path — `src/codegen.rs:4310,4345`). That
symbol lives in the C bridge `src/runtime_native.c`, which is only compiled and
linked when the program imports a **native module** (`has_native` in
`src/main.rs:207`). Result: **any** standalone program that uses `+` fails to
compile to `exe`/`shared`/`static`:

```
undefined reference to `__value_to_cstr'
```

Reproduce: `code build <file-with-a-plus>.code --target exe`.

**Fix options:** always compile+link the tiny C bridge (or a minimal subset) for
LLVM targets; or emit the number-only fast path for `+` when types are known and
only reference `__value_to_cstr` on the actual string path; or provide
`__value_to_cstr` as an always-linked intrinsic.

## 2. Default build target is inconsistent (`exe` vs `ir`)

- `src/main.rs:106,117` — `parse_target_flag` defaults to `"exe"`; help text says
  `exe … [default]`.
- `README.md:70` — table says `ir` is the default.
- `tests/llvm_codegen.rs::build_default_is_ir` — expects default → IR `.ll`.

These three disagree; `build_default_is_ir` fails in isolation as a result.
**Decision needed:** which is the intended default? Then align the other two.

## 3. llvm_codegen tests are not isolated (parallel flakiness)

Tests write to shared paths under `target/llvm/` and run in parallel, so the set
of failures changes run-to-run. Give each test a unique module name / output dir
(or a temp dir), so `cargo test --test llvm_codegen` is deterministic without
`--test-threads=1`.

## Acceptance criteria

- A standalone program using `+` compiles and runs via `--target exe`.
- Default target agrees across `main.rs`, README, and tests.
- `cargo test --test llvm_codegen` is green and order-independent.

## Effort

Medium. Item 1 is the substantive backend change; 2 is a one-line decision; 3 is
mechanical.

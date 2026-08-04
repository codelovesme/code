# T4 — Purge leftover `euglena` / `eug` naming from code and tests

- **Priority:** Low
- **Type:** Cleanup / consistency
- **Area:** `codegen.rs`, `runtime_native.c`, `tests/`

## Problem

The project was renamed Euglena → Code, but internal identifiers and test
fixtures still carry the old brand, which is confusing when reading the codebase
alongside the `code_*` public ABI.

### Evidence

- `src/codegen.rs` — exported WASM re-entry symbol `__euglena_dispatch` and
  `compile_euglena_dispatch_fn` (`codegen.rs:339`, `:353`, `:363`; comments at
  `:193`, `:345`, `:352`, `:976`).
- `src/runtime_native.c` — `eugvalue_to_cval` / `cval_to_eugvalue`
  (`runtime_native.c:223`, `:269`, and call sites).
- `tests/` — `tests/euglena/` directory and 8 `euglena_*.code` fixtures
  (`euglena_nucleus`, `euglena_logger`, `euglena_particles`,
  `euglena_persistence`, `euglena_cell_routing`, `euglena_cell_boot`,
  `euglena_transformer`, plus `tests/euglena/src/main.code`).

## Notes / risk

- `__euglena_dispatch` is referenced **only** inside `codegen.rs`; no host
  loader (`native_module.rs`, `wasm_module.rs`) looks it up by name, and the
  predecessor `euglena-language` repo contains no reference to it. It **is**,
  however, an *exported* wasm symbol, so treat the rename to `__code_dispatch`
  as a minor ABI change — safe, but call it out in release notes.
- The `euglena_*.code` fixtures are demo apps (cell/nucleus/organelle domain
  model). Renaming is cosmetic but touches test-runner expectations if any test
  matches on file names.

## Proposed change

- Rename `__euglena_dispatch` → `__code_dispatch`, `compile_euglena_dispatch_fn`
  → `compile_code_dispatch_fn`, and the C converters `eugvalue_to_cval` /
  `cval_to_eugvalue` → `codevalue_to_cval` / `cval_to_codevalue`.
- **Decision (resolved):** the `euglena_*` fixtures are a deliberate themed
  sample app (biology — Euglena organism with Cell/Nucleus/Organelle particles,
  linked via `link euglena/src/...`). They are an intentional example domain,
  **not** stale branding, and are kept as-is. Only leaked *implementation*
  identifiers are renamed.

## Acceptance criteria

- `grep -rniE 'eug(lena)?' src/ tests/` returns only intentional, documented
  hits (if any).
- Full test suite (`cargo test --workspace` + `code test`) passes after rename.

## Effort

Small–Medium (mechanical, but spans generated symbols + fixtures).

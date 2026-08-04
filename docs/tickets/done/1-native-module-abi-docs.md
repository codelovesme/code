# T1 — Fix stale native-module ABI documentation (`eug_*` → `code_*`, v1 → v2)

- **Priority:** High
- **Type:** Documentation / correctness
- **Area:** README, `native_module.rs` doc comments

## Problem

The public documentation for hand-writing native modules describes the **old
Euglena ABI**, which the current loader rejects. Anyone following the README to
write a `.so`/`.wasm` module by hand produces a module that fails to load.

### Evidence

Documented (stale):
- `README.md:404-448` — `eug_module_abi_version`, `eug_module_init`,
  `EugModuleDesc`, `eug_alloc`, `eug_fn_<idx>`, `eug_handler_<idx>`,
  `eug_number`/`eug_string`/`eug_object`.
- `README.md:407`, `README.md:414` — "must return 1".
- `src/native_module.rs:12` — doc comment `// must return 1`.

Actual runtime contract:
- `.so` loader looks up `code_module_abi_version` / `code_module_init`
  (`src/native_module.rs:442-457`).
- `.wasm` loader looks up `code_module_abi_version`, `code_module_init`,
  `code_alloc`, `code_handler_{idx}` (`src/wasm_module.rs:338-451`).
- Required ABI version is **2** — `CODE_ABI_VERSION = 2`
  (`src/native_module.rs:35`, `src/wasm_module.rs:108`), enforced at
  `src/native_module.rs:448`.

## Proposed change

1. Rewrite `README.md` "Native Module Linking" section to use the `code_*`
   symbol names and the `.wasm` export scheme (`code_fn_<idx>`,
   `code_handler_<idx>`, `code_alloc`).
2. Change every "must return 1" to "must return 2" (README + doc comment at
   `src/native_module.rs:12`).
3. Cross-check the C signatures in the README against the canonical header
   `tests/native_modules/code_abi.h` and keep them identical.

## Acceptance criteria

- `grep -rn 'eug_' README.md` returns nothing.
- No occurrence of "must return 1" anywhere in the repo.
- A native module hand-written from the README loads successfully.

## Effort

Small (docs only). No code behavior change.

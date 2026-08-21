# T6 — Restore dangling test fixtures + fix exe output name

- **Priority:** High
- **Type:** Test health / correctness
- **Area:** `tests/`, `src/main.rs`
- **Status:** Fixture portion DONE; residual llvm_codegen failures split out to T8.

## Problem

Two committed tests referenced `.code` fixtures that were never in the repo, so
`cargo test --workspace` was red on a clean checkout (CI included):

- `tests/json_module.rs` → `tests/json_module.code` (missing)
- `tests/llvm_codegen.rs` (10 call sites) → `tests/basic_assignment.code` (missing)

Separately, `code build --target exe` wrote `target/llvm/<name>.exe`, but README
(`README.md:71,84`), the tests, and Linux ELF convention all expect
`target/llvm/<name>` (no extension) — so every exe test failed on path lookup.

## What was done

- Added `tests/basic_assignment.code` — single-assignment + arithmetic, kept
  link-clean (no `+`; see T8) so the `--target exe` test links without the
  native runtime.
- Added `tests/json_module.code` — models a JSON-shaped document with nested
  objects/arrays and deep equality; runs under the interpreter.
- Fixed `src/main.rs`: executable output is now `target/llvm/<name>` (dropped the
  `.exe` suffix) and the CLI help text updated to match.

## Result

- `.code` language suite: **134/134 pass** (was 132; +2 new fixtures).
- `run_json_module`: **pass**.
- `llvm_codegen`: **16/17 pass in isolation** (was 6/17). The remaining failure
  and the parallel-run flakiness are pre-existing bugs unrelated to fixtures —
  tracked in **T8**.

## Acceptance criteria

- [x] `cargo test run_json_module` passes.
- [x] Missing fixtures restored; `code test` suite green.
- [x] `cargo test --test llvm_codegen` fully green — resolved in **T8**.

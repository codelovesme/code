# T2 — Single source of truth for the native-module ABI contract

- **Priority:** Medium
- **Type:** Refactor / maintainability
- **Area:** `native_module.rs`, `wasm_module.rs`, `crates/code-native`, C header

## Problem

The ABI version and the C-ABI struct layout are **hand-duplicated across four
places** with nothing enforcing agreement. They all say `2` today, but a bump
in one location silently diverges from the others and produces
load-time-only failures.

### Evidence — `CODE_ABI_VERSION` defined independently in:

- `src/native_module.rs:35` (`= 2`)
- `src/wasm_module.rs:108` (`= 2`)
- `crates/code-native/src/lib.rs` (`= 2`)
- `tests/native_modules/code_abi.h` (`ABI v2`, struct layout re-declared by hand)

The `#[repr(C)]` structs (`CodeValue`, `CodeField`, `CodeModuleDesc`, …) are
likewise re-declared in `native_module.rs`, `crates/code-native/src/lib.rs`, and
`code_abi.h`.

## Proposed change

Introduce a tiny **`code-abi`** crate (no dependencies, no LLVM) that owns:

- `pub const CODE_ABI_VERSION: u32`
- the `#[repr(C)]` struct definitions and tag constants

Then:

- `native_module.rs` / `wasm_module.rs` re-export from `code-abi` instead of
  redefining.
- `crates/code-native` depends on `code-abi` and re-exports it.
- Generate (or check-in with a test) `code_abi.h` from the Rust definitions so
  the C header cannot drift.

This crate is also the natural dependency for T3 (a parser/LSP frontend that
must not pull in LLVM).

## Acceptance criteria

- `CODE_ABI_VERSION` is defined exactly once in the workspace.
- A single edit bumps the version everywhere.
- A test fails if `code_abi.h` diverges from the Rust struct layout.

## Effort

Medium. Mechanical extraction; watch that `code-native` stays independently
compilable as a standalone `rlib` (it is compiled outside the host build).

## Resolution (implemented)

- New crate **`crates/code-abi`** (`publish = false`, MIT, `#![no_std]`, zero
  deps) now owns `CODE_ABI_VERSION`, the tag/target constants, every `#[repr(C)]`
  struct, the fn-type aliases, the `Send`/`Sync` impls, and `CodeValue::null()`.
- `src/native_module.rs` and `src/wasm_module.rs` re-use it (the former re-exports
  it so existing `native_module::Code*` paths still resolve); `crates/code-native`
  depends on it and re-exports via `pub use code_abi::*`.
- `CODE_ABI_VERSION = 2` now appears **exactly once** in Rust
  (`crates/code-abi/src/lib.rs`). Verified: workspace builds clean, native `.so`
  fixtures rebuilt with the new `code-native`, and all native-link tests pass —
  the ABI is byte-identical.

**Decision:** kept `code-abi` internal (`publish = false`), per owner.

### Follow-up (done)

The C header `tests/native_modules/code_abi.h` is still hand-maintained, but it is
now **guarded against drift**: `tests/abi_header_sync.rs` checks that the header's
`#define`s (version, tags, emit target) equal the Rust constants, and compiles a
C `_Static_assert` probe (seeded with Rust `size_of`/`offset_of`) so any struct
size/field-offset divergence fails the build. A machine-checkable
`#define CODE_ABI_VERSION 2` was added to the header. Both guards were verified to
fail on injected drift.

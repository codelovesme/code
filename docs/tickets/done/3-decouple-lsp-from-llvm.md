# T3 — Decouple `code-lsp` from LLVM/inkwell

- **Priority:** Medium
- **Type:** Architecture
- **Area:** `crates/code-lsp`, `src/lib.rs`

## Context

This is the concrete follow-up to the original "should the crates be in the
language repo?" question. Conclusion from investigation: keeping both crates
in-repo is fine — but `code-lsp`'s dependency shape is not.

## Problem

`code-lsp` depends on the entire language library:

```toml
# crates/code-lsp/Cargo.toml
code_lang = { path = "../..", package = "code" }
```

`code_lang` re-exports `codegen` (`src/lib.rs:2`), which pulls in
`inkwell` + **LLVM 17** (`Cargo.toml` root dependencies). The LSP only actually
uses `code_lang::parser` (`crates/code-lsp/src/main.rs:8`). Net effect: building
or installing the editor language server requires a full LLVM 17 toolchain it
never uses.

## Proposed change

Split the parser/AST/diagnostics surface into a lightweight, LLVM-free crate
(e.g. `code-frontend`: `ast` + `parser` + error types), and have both the main
`code` binary and `code-lsp` depend on it. Pairs naturally with the `code-abi`
crate from T2.

Alternative (smaller, if a full split is too invasive now): put the LLVM backend
behind a Cargo feature on `code_lang` (e.g. `llvm`, default-on for the binary),
and have `code-lsp` depend with `default-features = false`.

## Acceptance criteria

- `cargo build -p code-lsp` succeeds with **no LLVM installed** and no
  `LLVM_SYS_170_PREFIX` set.
- The `code` binary's build and behavior are unchanged.

## Resolution (implemented — feature-gate)

Investigation showed the full crate split was both blocked and misaimed:

- `ast` is coupled to `runtime`/`native_module` via the single
  `Statement::NativeImport` variant, so a clean `ast + parser` crate would have
  to drag `runtime` + `native_module` along — large, risky churn.
- **inkwell/LLVM is used by exactly one module, `codegen.rs`**, referenced only
  by `lib.rs` and the `main.rs` binary. The coupling blocking the split is
  *runtime*, not LLVM — orthogonal to the goal.

So (owner decision) the feature-gate was used instead:

- `inkwell` is now `optional`, enabled by a default `llvm` feature (`Cargo.toml`).
- `pub mod codegen;` is `#[cfg(feature = "llvm")]` in `src/lib.rs`.
- `code-lsp` depends on `code_lang` with `default-features = false`.

**Verified:**
- `cargo tree -p code-lsp` contains **no `inkwell`/`llvm-sys`** → builds without
  LLVM installed.
- `cargo build -p code --no-default-features --lib` compiles (codegen excluded).
- `cargo build --workspace` (binary keeps LLVM via default features) and the full
  test suite stay green.

### Follow-up (done)

A CI job `lsp-no-llvm` now builds `code-lsp` with **no LLVM installed**
(`LLVM_SYS_170_PREFIX` blanked) and fails if `cargo tree -p code-lsp` shows
`inkwell`/`llvm-sys` — locking the decoupling in place. The true `code-frontend`
crate split remains a possible future refactor if `ast`/`runtime` are ever
decoupled.

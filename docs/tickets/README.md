# Backlog

Findings from the repo audit (native-module ABI, crate layout, docs), one file
per ticket.

| ID | Title | Priority | Status |
|----|-------|----------|--------|
| [T1](T1-native-module-abi-docs.md) | Fix stale native-module ABI docs (`eug_*` → `code_*`, v1 → v2) | High | done |
| [T2](T2-abi-version-single-source.md) | Single source of truth for the ABI contract (`code-abi` crate) | Medium | done |
| [T3](T3-decouple-lsp-from-llvm.md) | Decouple `code-lsp` from LLVM/inkwell (feature-gate) | Medium | done |
| [T4](T4-purge-euglena-naming.md) | Purge leftover `euglena`/`eug` naming | Low | done |
| [T5](T5-readme-completeness.md) | README completeness and polish | Low | done |
| [T6](T6-test-suite-fixtures.md) | Restore dangling test fixtures + fix exe output name | High | done |
| [T7](T7-readme-semantic-mismatches.md) | README documents semantics the impl lacks (reassignment, `${}`) | High | done |
| [T8](T8-llvm-backend-and-test-isolation.md) | `+` link bug, default-target disagreement, llvm test isolation | High | done |
| [T9](T9-ast-spans-for-runtime-diagnostics.md) | Located errors via AST spans — `run` + `build`, single- & multi-file (expression-level dropped) | Medium | done |
| [T10](T10-negative-number-literal.md) | Negative number literals (`-5`) don't parse | Medium | done |
| [T11](T11-ditch-function-call-syntax-plan.md) | [PLANNING] Retire `name(args)` call syntax; move built-ins to handlers | High | decided |
| [T12](T12-core-handlers-implementation.md) | Implement `to core` handler dispatch; remove `Expression::Call` | High | done |

`cargo test --workspace` and the `.code` suite are fully green.

**Design decision on record:** Code has no user-defined functions and no
function value — reusable logic exists only as handlers (particle dispatch).
The README's former "Functions" section documented a feature that never
existed; it has been removed. T11 decided (full retirement, no `name(args)`
sugar survives): `timestamp`/`length` move to `emit X to core get result`.
T12 implemented it — `Expression::Call` is fully removed from the language
(`grep -rn 'Expression::Call' src/` is empty). The `.wasm` ABI's dead
function-export slot was investigated too: shrinking it would be a breaking
wire-format change for no gain (see T12's resolution), so it stays as
reserved/zeroed padding, honestly relabeled instead of removed.

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
| [T9](T9-ast-spans-for-runtime-diagnostics.md) | Located errors via AST spans — `run` + `build`, single-file done; multi-file + expression-level deferred | Medium | partial |

`cargo test --workspace` and the `.code` suite are fully green. T9 is deferred;
its interim mitigation (richer runtime error messages) shipped.

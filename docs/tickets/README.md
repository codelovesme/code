# Backlog

Findings from the repo audit (native-module ABI, crate layout, docs) plus the
distribution roadmap, one file per ticket.

**Layout:** completed tickets live in [`done/`](done/); everything still
outstanding lives under its priority — [`high/`](high/), [`medium/`](medium/),
`low/` (created once something lands there). A ticket's priority is recorded
inside the file itself (`**Priority:**` line) even after it moves to `done/`,
so nothing is lost in the move — `done/` isn't priority-sorted because, once
finished, priority no longer matters for it. Numbers are stable identifiers
(never reused), not folder-relative — ticket 12 stays "12" whether it's in
`done/` or, hypothetically, moved back.

## Done

| # | Title |
|---|-------|
| [1](done/1-native-module-abi-docs.md) | Fix stale native-module ABI docs (`eug_*` → `code_*`, v1 → v2) |
| [2](done/2-abi-version-single-source.md) | Single source of truth for the ABI contract (`code-abi` crate) |
| [3](done/3-decouple-lsp-from-llvm.md) | Decouple `code-lsp` from LLVM/inkwell (feature-gate) |
| [4](done/4-purge-euglena-naming.md) | Purge leftover `euglena`/`eug` naming |
| [5](done/5-readme-completeness.md) | README completeness and polish |
| [6](done/6-test-suite-fixtures.md) | Restore dangling test fixtures + fix exe output name |
| [7](done/7-readme-semantic-mismatches.md) | README documents semantics the impl lacks (reassignment, `${}`) |
| [8](done/8-llvm-backend-and-test-isolation.md) | `+` link bug, default-target disagreement, llvm test isolation |
| [9](done/9-ast-spans-for-runtime-diagnostics.md) | Located errors via AST spans — `run` + `build`, single- & multi-file (expression-level dropped) |
| [10](done/10-negative-number-literal.md) | Negative number literals (`-5`) don't parse |
| [11](done/11-ditch-function-call-syntax-plan.md) | [PLANNING] Retire `name(args)` call syntax; move built-ins to handlers (decided) |
| [12](done/12-core-handlers-implementation.md) | Implement `to core` handler dispatch; remove `Expression::Call` |
| [13](done/13-release-workflow.md) | Release workflow: GitHub Releases on tag push (Linux x86_64) |

`cargo test --workspace` and the `.code` suite are fully green.

## Active — High priority

| # | Title |
|---|-------|
| [14](high/14-install-script.md) | Install script (`curl \| sh`) |

## Active — Medium priority

| # | Title |
|---|-------|
| [15](medium/15-publish-code-native-crates-io.md) | Publish `code-native` to crates.io |
| [16](medium/16-vscode-extension-consolidation-and-publish.md) | Consolidate VS Code extension into this repo; publish to Marketplace |

## Active — Low priority

_(none yet)_

---

**Distribution roadmap (approved 2026-07-31):** the language had zero
distribution before this — no release binaries, no installer, no published
packages, no docs site, no playground. Tickets 13–16 are Phase 1 (release
binaries, installer, `code-native` on crates.io, VS Code Marketplace) of a
3-phase plan; Phase 2 (docs site, browser playground) and Phase 3 (multi-
platform binaries, package registry) are planned but not yet ticketed.
Ticket 13 (release workflow) is done — pushing a `v*` tag now produces a
GitHub Release with a standalone Linux x86_64 binary. Two remaining Phase 1
items are credential-gated to the repo owner (crates.io token, VS Code
publisher PAT).

**Design decision on record:** Code has no user-defined functions and no
function value — reusable logic exists only as handlers (particle dispatch).
The README's former "Functions" section documented a feature that never
existed; it has been removed. Ticket 11 decided (full retirement, no
`name(args)` sugar survives): `timestamp`/`length` move to
`emit X to core get result`. Ticket 12 implemented it — `Expression::Call` is
fully removed from the language (`grep -rn 'Expression::Call' src/` is
empty). The `.wasm` ABI's dead function-export slot was investigated too:
shrinking it would be a breaking wire-format change for no gain (see ticket
12's resolution), so it stays as reserved/zeroed padding, honestly relabeled
instead of removed.

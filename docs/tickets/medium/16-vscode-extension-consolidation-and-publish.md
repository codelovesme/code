# T16 — Consolidate VS Code extension into this repo; publish to Marketplace

- **Priority:** Medium
- **Type:** Distribution (Phase 1 of the distribution roadmap approved 2026-07-31)
- **Area:** `editors/vscode/` (new)
- **Credential-gated:** publish step needs the owner's Azure DevOps publisher
  account + PAT.

## Problem

The VS Code extension (TextMate grammar + LSP client for `.code` files)
exists, but not in this repo — it lives at `../cdlvsm/code-vscode` and is
duplicated, byte-for-byte, across 10+ sibling `cdlvsm-*` projects. It has
never been published to the VS Code Marketplace, so no one can just search
"Code language" and install it.

## Decision on record (owner, 2026-07-31)

Move the canonical source **into this `code` repo**, at `editors/vscode/`
(matching the Rust ecosystem convention — rust-analyzer uses `editors/code/`)
— language and extension live in one repo, one version cycle, one source of
truth.

**Explicitly out of scope for this ticket:** touching/deleting the duplicated
copies in the other `cdlvsm-*` repos. Those are separate projects; this
ticket only *adds* the canonical copy here. Cleanup elsewhere is a separate,
later decision for the owner to make deliberately, not a side effect of this
migration.

## Alternatives considered and rejected (2026-08-01 discussion)

- **Separate repo for the extension** (clangd/`vscode-clangd` model — LSP in
  the compiler monorepo, client in its own repo). Rejected for this project's
  current shape: `code-lsp` depends on `code_lang` via a path dependency
  (`code_lang = { path = "../.." }`), so the client/server pair here is closer
  to the rust-analyzer model (LSP + its VS Code extension share one repo,
  separate from the language's own compiler repo) than to clangd's, whose
  separation is driven by things this project doesn't have yet: multiple
  competing clients (Microsoft's `cpptools` vs `vscode-clangd`), a much larger
  contributor base needing isolation from the core repo, and an independent
  release cadence. Revisit if any of those actually materialize.
- **`code lsp` subcommand wrapping `code-lsp`** (so users only ever type
  `code`, never `code-lsp` directly). Rejected: this ticket's own server-path
  resolution step already has the extension find/bundle `code-lsp` directly;
  a human essentially never invokes `code-lsp` by hand, so the wrapper adds
  surface for a problem nobody has. Cheap to add later (a few lines, fully
  reversible) if that changes.

## Proposed change

1. Copy the most current version of `../cdlvsm/code-vscode` into
   `editors/vscode/` in this repo (package.json, syntaxes/code.tmLanguage.json,
   language-configuration.json, src/extension.ts).
2. Update `src/extension.ts`'s server-path resolution so it can find this
   repo's `code-lsp` binary sensibly (bundled with the extension via the
   release tarball from T13, or a documented "server path" setting — decide
   during implementation, not pre-specified here).
3. Bundle with esbuild (per current VS Code extension publishing guidance) so
   `vsce package`/`vsce publish` work with `--no-dependencies`.
4. `vsce package` → install the `.vsix` locally in a clean VS Code profile,
   confirm hover/completion/format/semantic-tokens all work against a real
   `.code` file.
5. **The owner sets up an Azure DevOps publisher + PAT, runs `vsce login` and
   `vsce publish`** — I cannot create this account or hold this credential.

## Acceptance criteria

- `editors/vscode/` exists in this repo and is the canonical source going
  forward (README/CONTRIBUTING should say so).
- Packaged `.vsix` installs and works end-to-end in a clean VS Code profile.
- Extension is live on the VS Code Marketplace (owner-executed final step).

## Effort

Medium — mostly relocation + bundling/packaging plumbing; the extension's own
functionality already exists and works.

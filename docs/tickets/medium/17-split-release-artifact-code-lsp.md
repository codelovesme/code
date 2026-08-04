# 17 — Split release artifacts: `code` Runtime / `code` SDK / `code-lsp`

- **Priority:** Medium
- **Type:** Distribution (Phase 1 follow-up; amends T13, informs T16; absorbs
  T18's packaging-time naming decision)
- **Area:** `.github/workflows/release.yml`, `install.sh`, `README.md`,
  `Cargo.toml` (`[profile.release]`)

**Scope note (2026-08-01):** this ticket grew from a two-way split
(`code`/`code-lsp`, by audience) to a three-way split once T18 made a second,
orthogonal axis real: the `code` binary itself now ships as two capability
tiers (Runtime / SDK), both still named `code`. See "Runtime vs SDK" below.

## Problem

`code` and `code-lsp` are independent at runtime in both directions: `code`'s
source never references `code-lsp`, and `code-lsp` never shells out to or
requires the `code` binary (confirmed — no `Command::new`/subprocess call to
`code` anywhere in `crates/code-lsp/src`). Despite that, T13's release
workflow packages both into a single tarball
(`code-<version>-x86_64-linux.tar.gz`), and `install.sh` installs both
unconditionally (`install.sh:65` — `cp "$stage_dir/code" "$stage_dir/code-lsp"
"$BIN_DIR/"`).

This forces every consumer to download both binaries regardless of which one
they actually need:
- A CLI-only user (`code build`/`run`/`fmt`/`test`, no editor integration)
  downloads `code-lsp` weight they'll never execute.
- An editor-only consumer (T16's VS Code extension, or a Neovim/Helix/Emacs
  user configuring the language server manually) has no way to fetch
  `code-lsp` on its own — they'd have to pull the combined tarball just to
  get one binary out of it.

This also leaves T16's "server-path resolution" step (how the extension
locates/bundles `code-lsp`) without a clean release asset to point at.

## Precedent

This mirrors how `rust-analyzer` actually ships: its prebuilt binaries are
published as their own standalone GitHub Release assets, independent of any
`rustc`/CLI tarball. VS Code, Neovim (`mason.nvim`), Helix, and Emacs all fetch
that asset directly — no bundling, no combined download.

## Runtime vs SDK (folded in from T18)

`code` is built twice with different `--features`, producing a binary named
`code` in both cases (same name, capability differs — the `dotnet`
Runtime/SDK model; see T18's "Naming decision"):

- **`code-runtime-<version>-x86_64-linux.tar.gz`** — `code` built
  `--no-default-features` (T18). Has `run`/`fmt`/`test`; `code build` prints
  T18's "install the SDK" error. Measured ~4.5M stripped / ~1.6M compressed.
- **`code-sdk-<version>-x86_64-linux.tar.gz`** — `code` built with `llvm`
  (today's default, unchanged). Everything, including `build`. ~42M stripped
  / ~22-25M compressed.

## Proposed change

1. `release.yml`: package **three** tarballs instead of one, all on the same
   GitHub Release (same tag):
   - `code-runtime-<version>-x86_64-linux.tar.gz` — LLVM-free `code` +
     `LICENSE` + `README.md`.
   - `code-sdk-<version>-x86_64-linux.tar.gz` — full `code` (with `build`) +
     `LICENSE` + `README.md`.
   - `code-lsp-<version>-x86_64-linux.tar.gz` — `code-lsp` binary + `LICENSE`.
2. **Strip both `code` builds and `code-lsp`**: add `[profile.release] strip
   = true` to the root `Cargo.toml`. Measured impact: modest but free —
   combined unstripped tarball (today's shape) is ~25M compressed, stripped
   ~22M (~12%; LLVM's static data was already highly compressible, so this
   is smaller than it sounds — still a one-line, zero-risk win, not the
   headline size story).
3. `install.sh`: defaults to the **SDK** tarball (today's full behavior,
   unchanged default) — but see the not-yet-written distribution/website
   ticket for whether/how to let it fetch Runtime instead. Drop the
   `code-lsp` cp/chmod lines and the "(+ `code-lsp`)" wording in its header
   comment either way — this script is for CLI users; it should not carry
   LSP weight regardless of which `code` tier it installs.
4. `README.md`: document `code-lsp`'s standalone asset URL pattern for editor
   tooling / manual LSP setup (Neovim/Helix/Emacs users), and the
   Runtime-vs-SDK choice for CLI users, separate from the "Installing"
   section's `install.sh` instructions.
5. T16: when implemented, point the extension's server-path resolution at the
   `code-lsp-*.tar.gz` asset directly.

## Explicitly out of scope / rejected alternative

**Publishing `code-lsp` to crates.io** (like `code-native`) was considered and
rejected:

- **Blocked today, mechanically**: `code-lsp` depends on `code_lang` via a
  path dependency (`code_lang = { path = "../.." }`), and crates.io requires
  every dependency of a published crate to be registry-resolvable. The root
  `code` package isn't published. `code-native` solved the analogous problem
  for `code-abi` by vendoring a small, stable ABI surface with a drift-guard
  test — but vendoring the entire parser/AST/interpreter/formatter (large,
  actively changing — see T9–T12) to work around the same constraint would
  create a second copy of the language's core logic that must be kept in
  sync by hand. A missed sync means `code` and `code-lsp` disagree on
  diagnostics/parsing — undermining the LSP's whole purpose.
- **Wouldn't solve the actual problem even if done**: crates.io / `cargo
  install` compiles from source on the consumer's machine (requires a Rust
  toolchain, network access to the registry, real local compile time). The
  goal here is the opposite — a prebuilt binary a user or editor plugin can
  fetch and run with zero build step. GitHub Release assets already deliver
  that for free with existing tooling (T13's workflow).
- **Premature commitment**: publishing `code_lang` itself (the only way to
  give `code-lsp` a real registry dependency instead of vendoring) means
  committing to semver stability for the parser/AST/interpreter API — while
  the language is still under active structural redesign (T9 AST spans,
  T11/T12 removing `Expression::Call` entirely). Revisit if/when the
  language's core API stabilizes; not blocking for this ticket.

## Acceptance criteria

- Pushing a `v*` tag produces three separate downloadable tarballs on the same
  GitHub Release: `code-runtime-*`, `code-sdk-*`, `code-lsp-*`.
- Both `code` tiers are internally named `code` (no `[[bin]]` rename); tier
  identity lives in the tarball/package name only.
- `install.sh` never touches `code-lsp`.
- README documents how to fetch `code-lsp` and `code-runtime` standalone.
- Release binaries are stripped.

## Effort

Small — repackaging step in an existing workflow (build twice with different
`--features`, three `tar czf` calls instead of one), plus the strip profile
line and trimming `install.sh`. No source/logic changes to any binary beyond
T18's `#[cfg]` gating.

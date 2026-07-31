# 15 [READY — blocked on owner's `cargo publish`] — Publish `code-native` to crates.io

- **Priority:** Medium
- **Type:** Distribution (Phase 1 of the distribution roadmap approved 2026-07-31)
- **Area:** `crates/code-native/Cargo.toml`
- **Credential-gated:** needs the owner's crates.io account + API token.

## Problem

`code-native` — the MIT-licensed helper crate for authoring native `.so`/
`.wasm` modules — is only reachable via a git path today
(`code-native = { git = "..." }`). Anyone wanting to write a native module for
this language has to know the exact repo and path.

## Decision on record (owner, 2026-07-31)

`code-abi` **stays internal** (`publish = false`) — the T2 decision holds.
Only `code-native` is published; it already re-exports everything `code-abi`
provides, so external authors get full ABI access transitively without a
second crates.io dependency to manage.

## Proposed change

1. Confirm `crates/code-native/Cargo.toml` has crates.io-required metadata
   (`description`, `license`, `repository` — already present per earlier
   session work; double-check `readme`, `keywords`, `categories` for
   discoverability).
2. `cargo publish --dry-run -p code-native` to catch packaging issues
   (e.g. the `code-abi` path dependency — crates.io requires path deps to
   also be published or version-pinned to a published version; since
   `code-abi` is staying internal, this needs resolving — likely by
   **vendoring/inlining `code-abi`'s contents into `code-native`** at publish
   time, or restructuring so `code-native` doesn't have an unpublishable path
   dependency. Needs a decision before this ships — flag as an open
   implementation question, not pre-decided here.)
3. Once dry-run is clean: **the owner runs `cargo login` with their own
   token, then `cargo publish -p code-native`** — I cannot do this step
   myself.

## Acceptance criteria

- `cargo publish --dry-run -p code-native` succeeds.
- Crate is live on crates.io with correct metadata and a working example in
  its docs.rs-rendered README.

## Effort

Small–Medium — the `code-abi` path-dependency question (item 2 above) is the
only real unknown; everything else is metadata/process.

## Resolution so far

The path-dependency question (item 2) is resolved: `code-abi` moves to
`code-native`'s `[dev-dependencies]` (fine for publishing — dev-deps aren't
part of the published package), and `crates/code-native/src/abi.rs` is a
vendored, mechanically-kept-in-sync copy of `code-abi`'s contract (same
public API, `mod abi; pub use abi::*;` instead of re-exporting the external
crate). A new drift guard, `crates/code-native/tests/abi_in_sync.rs`,
compares constants and every struct's size/align/field-offset between the two
copies in pure Rust — verified it actually catches drift (injected a constant
mismatch and a field-offset mismatch, confirmed both fail, reverted).

Also added `readme`/`keywords`/`categories` to `Cargo.toml` plus a
crate-level `README.md` (reusing the existing quick-start doc-comment
example) for crates.io discoverability, per item 1.

**Verified:**
- `cargo tree -p code-native -e normal` — empty, zero runtime dependencies.
- `cargo publish --dry-run -p code-native` against the actual committed
  state (not `--allow-dirty`) — packages, and *verifies* by compiling the
  extracted tarball standalone with no workspace context, confirming it
  would genuinely build for someone who `cargo add code-native`'d it.
- Host-side `code-abi` usage (`native_module.rs`, `wasm_module.rs`) is
  completely untouched — only `code-native` changed.
- Full workspace suite green throughout.

**Remaining — needs the owner, credential-gated:**
```bash
cargo login          # with your own crates.io API token
cargo publish -p code-native
```
Everything up to this point is done and verified; this is the one command
left.

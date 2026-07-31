# T15 — Publish `code-native` to crates.io

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

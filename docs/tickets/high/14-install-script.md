# T14 — Install script

- **Priority:** High
- **Type:** Distribution (Phase 1 of the distribution roadmap approved 2026-07-31)
- **Area:** `install.sh` (new, repo root), `README.md`
- **Depends on:** T13 (needs a GitHub Release to download from)

## Problem

Even once T13 ships release binaries, users still have to find the right
release, download it, extract it, and put it on `PATH` manually.

## Proposed change

`install.sh` at repo root, rustup-style:
- Detects OS/arch (initially just validates it's Linux x86_64 — the only
  platform T13 ships; error clearly otherwise, pointing at "build from
  source" instructions).
- Downloads the latest (or a pinned, via `$CODE_VERSION`) GitHub Release
  tarball for this repo.
- Extracts `code`/`code-lsp` into `~/.local/bin` (or `$PREFIX/bin` if set),
  creating the directory if needed.
- Prints a PATH reminder if `~/.local/bin` isn't already on `PATH`.

Document in README's "Building" section as the recommended path for users who
just want the binary:

```bash
curl -sSf https://raw.githubusercontent.com/codelovesme/code/main/install.sh | sh
```

Keep the existing `cargo build` instructions for contributors/source builds.

## Acceptance criteria

- Running the script on a clean machine/container with nothing preinstalled
  results in a working `code` on `PATH`.
- Clear error message (not a silent failure) on unsupported platforms.

## Effort

Small.

## Verification

Test inside a clean container: `docker run --rm -it ubuntu:24.04` with
nothing preinstalled, run the install command, confirm `code --version`
works.

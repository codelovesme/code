# 14 [DONE] — Install script

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

## Resolution (implemented)

Shipped as `install.sh` at the repo root, matching the proposed design
exactly (platform check, `$CODE_VERSION` pin, `$PREFIX` override, PATH
reminder). README's new "Installing" section (ahead of "Building") documents
it as the recommended path for users who just want the binary.

**Real GitHub Release doesn't exist yet** (T13's `Create GitHub Release` step
has only run in dry-run mode so far — see T13's resolution), so the actual
`https://github.com/.../releases/...` download couldn't be exercised against
a live release. Verified everything that *can* be verified without one:

- A byte-for-byte diff against a copy of the script with only its two URLs
  redirected to a local HTTP server confirmed the extraction/install/PATH
  logic is identical to what ships.
- **Real clean-container test** (`docker run --rm ubuntu:24.04`, genuinely
  nothing preinstalled): running the script bare correctly failed with
  `error: 'curl' is required but not found on PATH` (`curl` isn't in the base
  Ubuntu image) — the `need()` guard's exact job, exit 1, no cryptic
  failure. With `curl` installed (matching a realistic user machine — curl is
  near-universal), the script downloaded, extracted, installed both binaries
  to `~/.local/bin`, printed the correct version, and printed the PATH
  reminder (verified suppressed when `$BIN_DIR` is already on `PATH`).
  Confirmed the installed binary actually runs a `.code` program correctly,
  not just `--version`.
- Unsupported-platform error path verified by direct logic check (no non-x86
  machine available to trigger it via `uname` for real).

**Not yet verified**: the live download from a real published GitHub Release
— gated on the repo owner cutting a real `v*` tag (T13's same open item).

## Effort (actual)

Small, as estimated.

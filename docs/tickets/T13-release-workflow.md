# T13 — Release workflow: GitHub Releases on tag push

- **Priority:** High
- **Type:** CI / distribution (Phase 1 of the distribution roadmap approved 2026-07-31 — see T13–T16 for the full Phase 1 set)
- **Area:** `.github/workflows/release.yml` (new)

## Problem

No packaged output of this repo ever leaves the build machine. Trying the
language requires cloning the repo and installing LLVM 17 + lld from source —
real friction for anyone who just wants to run `code` once.

## Proposed change

New workflow, triggered on `v*` tag push, mirroring `ci.yml`'s existing LLVM 17
setup on `ubuntu-latest` (no new toolchain work — same
`LLVM_SYS_170_PREFIX`, same `apt-get install llvm-17 llvm-17-dev
libpolly-17-dev clang-17 lld-17`):

1. Build `code` and `code-lsp` in `--release` profile.
2. Package into a tarball: `code-<version>-x86_64-linux.tar.gz` (binaries +
   `LICENSE` + `README.md`).
3. Create a GitHub Release for the tag and attach the tarball (via
   `gh release create` or `softprops/action-gh-release`).

**Scope: Linux x86_64 only.** LLVM/lld cross-compilation to macOS/Windows is
real added complexity — deferred to Phase 3 of the roadmap, not blocking here.

## Acceptance criteria

- Pushing a `v*` tag produces a GitHub Release with a downloadable tarball.
- The extracted `code` binary runs standalone (`./code --version`) with no
  LLVM installed on the target machine (release builds statically link
  what's needed the same way `code build --target exe` already does for
  `.code` programs — verify this holds for the compiler binary itself, not
  just its output).

## Effort

Small — new workflow file only, reusing CI's existing LLVM setup verbatim.

## Notes

Verify via a throwaway tag (e.g. `v0.2.0-test1`) on a branch before tagging a
real release, per the roadmap's verification plan.

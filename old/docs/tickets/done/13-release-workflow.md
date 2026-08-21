# 13 [DONE] — Release workflow: GitHub Releases on tag push

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

## Resolution (implemented)

Shipped as `.github/workflows/release.yml`, exactly as proposed, plus a
`workflow_dispatch` trigger for dry-runs (builds + uploads an Actions
artifact but skips `Create GitHub Release`, gated on `github.ref_type ==
'tag'`) so the packaging can be exercised without cutting a real tag.

- **Fixed an independent bug this surfaced**: `src/main.rs`'s `VERSION`
  constant was hardcoded to `"Code v0.1"` while `Cargo.toml` already said
  `0.2.0` — `code --version` was lying. Now `concat!("Code v",
  env!("CARGO_PKG_VERSION"))`, so it can't drift from `Cargo.toml` again.
- **Verified the "no LLVM installed" acceptance criterion is actually true**
  before writing the workflow (not just assumed): `ldd target/release/code`
  has no `libLLVM` entry — inkwell/llvm-sys statically link LLVM with this
  repo's setup. Remaining dynamic deps (`libc`, `libstdc++`, `libffi`,
  `libz`, `libzstd`, `libtinfo`) are present on any standard Linux
  desktop/server. The workflow encodes this as a hard `ldd` gate (fails the
  build if a future dependency change reintroduces a dynamic LLVM link) —
  not just a one-time manual check.
- **Verified end-to-end twice**: once locally (built `--release`, packaged
  the exact tarball layout the workflow produces, extracted it fresh, ran
  `code --version` → `Code v0.2.0`, confirmed via `ldd`), then for real via
  `gh workflow run release.yml --ref main` (`workflow_dispatch`) — full run
  succeeded in 2m29s
  ([run 30666592066](https://github.com/codelovesme/code/actions/runs/30666592066)),
  produced artifact `code-dev-<sha>-x86_64-linux.tar.gz`, `Create GitHub
  Release` correctly skipped (not a tag push).
- **Not yet verified**: the actual `Create GitHub Release` step (only runs on
  a real `v*` tag push) — cutting a real tag is a visible, public action
  reserved for the repo owner to trigger deliberately, not done as part of
  this ticket's verification.

## Effort (actual)

Small, as estimated — one new workflow file, one one-line unrelated bug fix
found along the way.

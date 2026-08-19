# 33 — [PLANNING] Extract `cdlvsm` to its own repo as a real Rust CLI

- **Priority:** Medium
- **Type:** Distribution / Ecosystem Architecture
- **Area:** new repo `codelovesme/cdlvsm-cli`; `cdlvsm`, `install.sh`,
  `README.md`, `.github/workflows/pages.yml`, `site/downloads.html`,
  `docs/tickets/done/32-...` in this repo

Status: **Planned, not started.** Full design below — this ticket exists so
the plan survives across machines/sessions; nothing has been implemented
yet (no new repo created, no code-repo files touched).

## Problem

T32 shipped `cdlvsm` as a POSIX-`sh` package manager living inside this
repo (`install`/`list`/`uninstall` for `code`, with `euglena` stubbed as
"not published yet"). The user now wants `cdlvsm` to also act as a
**dispatcher** — `cdlvsm code run abc.code`, `cdlvsm euglena up` —
transparently forwarding to whichever tool got installed, and wants it in
its own repo, since it's meant to serve every first-party CLI tool, not
just `code`.

Investigation this session found `euglena` is real, not hypothetical: it
lives in `codelovesme/euglena-platform` (git remote; locally checked out
at `~/git/test/cdlvsm` on the machine this was planned on — that path is
just a local nickname predating this work, not the real repo name), a
substantial monorepo (`euglena-cli`, native/web runtimes, 24 organelle
capability crates, `code-vscode`) that `code` was originally extracted
*out of* (that repo's own ticket 014). Confirmed via direct investigation,
not assumption: **euglena has zero release infrastructure today** — no
release workflow, no GitHub Releases, not on crates.io, no coherent
product version. `euglena up` is not a real `euglena-cli` subcommand
(closest real thing is `dev.sh up`, a local dev-stack script) — treat it
as illustrative future naming, not a contract to satisfy today. So
`cdlvsm install euglena` has nothing to fetch and must keep failing
clearly, exactly as the shell prototype already does.

User decisions made this session: new repo **`codelovesme/cdlvsm-cli`**
(confirmed name), implementation **Rust** (confirmed — matches every other
real tool in this ecosystem: `code`, `euglena-cli`, `code-lsp`).

## Proposed change

### New repo: `codelovesme/cdlvsm-cli`

Single-crate Rust binary, `[[bin]] name = "cdlvsm"`. Public visibility
(must be anonymously `curl`-installable, matching `code`'s own pattern) —
confirm `euglena-platform`'s visibility once on the `codelovesme` gh
account (whichever machine executes this: check `gh auth status` first —
this session's local gh CLI kept silently reverting to
`fedailyuseinexperiencedata`, which can't even see `euglena-platform`),
but default `cdlvsm-cli` to public regardless of what `euglena-platform`
turns out to be, since the CLI itself is a distribution tool.

**Dispatch design (`src/main.rs`):** manual `std::env::args()` matching —
no `clap` — mirroring `code`'s own top-level dispatch style (`src/main.rs`'s
`match args[1].as_str() { ... }`), because a static clap subcommand enum
doesn't fit "known built-in, OR an arbitrary installed package name with
arbitrary trailing args."

- Built-ins: `install <pkg> [--runtime] [--link]`, `uninstall <pkg>`,
  `list`, `-h`/`--help`/`help`, `-v`/`--version`. These are permanently
  reserved package names — document this explicitly (README + a comment
  above the dispatch match).
- Anything else in `args[1]` that does **not** start with `-` is treated
  as a package name: resolve `$PREFIX/share/cdlvsm/packages/<name>/
  current/<name>` (binary name == package name, true for both `code` and
  `euglena`). If it exists, dispatch via `exec()`; if not, error
  "`<name>` is not installed — run `cdlvsm install <name>` first."
  Anything starting with `-` (a typo'd flag) must never fall into the
  package-lookup path — route straight to an "unknown command" error
  instead, so `cdlvsm -x` doesn't produce the nonsensical "run `cdlvsm
  install -x`".
- Passthrough uses `std::os::unix::process::CommandExt::exec()` (not
  spawn+wait) on an **absolute** path — this is the process-replacement
  syscall, so it's not "avoids one extra process," it's what makes exit
  codes and signal handling (Ctrl-C, etc.) exactly correct for free; a
  spawn+wait alternative would need manual `ExitStatusExt::signal()`
  translation to avoid silently reporting exit code `1` for a
  signal-killed child. No stdio redirection — plain `Command::new(path)`
  inherits stdin/stdout/stderr, which is the whole point; comment this
  explicitly so a future edit doesn't "improve" it by capturing output.
  `.exec()` only returns on failure (not-executable, exec-format-error,
  etc.) — handle that `Result` with a clean `eprintln!`+`exit(1)`, don't
  let it panic.
- Uniform error contract everywhere: `eprintln!` + `std::process::exit(1)`,
  never `.unwrap()`/`.expect()` in a reachable path (that panics with exit
  101 + backtrace, breaking the shell prototype's "one clean line to
  stderr" contract).
- Tolerate corrupted install state the same way the shell version already
  does — `list` silently skips a package dir with no/broken `current`
  symlink; `uninstall` only requires the package dir to exist. Don't
  tighten this as part of the port.

**Package registry (`src/package.rs` or similar):** `enum Package { Code,
Euglena }`, consulted only by `install`/`uninstall`/`list` (dispatch
itself is purely filesystem-based, doesn't touch this enum).
`Package::Code::install()` ports the proven shell logic 1:1:

- Resolve version: `CDLVSM_CODE_VERSION` env var, else GitHub API `latest`
  tag lookup (keep the existing `grep`/`sed` tag-name extraction as-is —
  it's already proven; don't "upgrade" it to a JSON dep as part of this
  port).
- Download `code-{sdk,runtime}-<tag>-x86_64-linux.tar.gz` from
  `codelovesme/code` releases, extract, `chmod +x` the binary explicitly
  (don't trust the tarball to preserve the exec bit).
- Filesystem layout, unchanged from the shell prototype: `$PREFIX/share/
  cdlvsm/packages/<pkg>/<version>/`, a `current` symlink to the active
  version, `$PREFIX/bin/cdlvsm-<pkg>` always created, `$PREFIX/bin/<pkg>`
  only with `--link` (opt-in, avoids the VS Code `code` collision by
  default). `PREFIX` env var, default `$HOME/.local`.
- Download/extract shells out to `curl`/`tar` via `std::process::Command`
  rather than adding `ureq`/`reqwest`+`tar`+`flate2` crates — matches this
  org's existing dependency-minimalism (`code`'s own `Cargo.toml` has 6
  deps, zero HTTP/archive crates) and is lower-risk for a straight port of
  already-tested behavior. Put the two operations behind small named
  functions (`fn download(url, dest)`, `fn extract(tarball, dest)`) rather
  than inlining `Command::new("curl")` at every call site — cheap seam for
  later.
- `Package::Euglena::install()` stays a clean, hardcoded error ("not
  published yet") — no speculative fetch logic for an artifact that
  doesn't exist.
- `uninstall`'s safety check ports exactly: only remove `$BIN_DIR/
  cdlvsm-<pkg>` / `$BIN_DIR/<pkg>` if each is a symlink whose target
  resolves under that package's own directory — never touch a same-named
  file cdlvsm didn't create (e.g. VS Code's real `code`).

**Automated tests (new — the shell prototype only had manual
verification):** integration tests (`tests/`, `assert_cmd`-style
`Command::new(env!("CARGO_BIN_EXE_cdlvsm"))` against a `tempfile`-built
fake `$PREFIX`, matching the pattern in this repo's own
`tests/format_cli.rs`):

- Real install of `code` against the actual `codelovesme/code` GitHub
  release (mirrors the manual verification already done for the shell
  version) — `install`, `--link`, `list`, dispatch passthrough
  (`cdlvsm code --version` forwards correctly), `uninstall`.
- **Uninstall-safety regression test** (the single most important
  property to protect going forward): a plain non-symlink file standing
  in for a foreign `code` binary must survive `cdlvsm uninstall code`
  untouched. This was only manually verified for the shell version — it
  needs to be a real `#[test]` running in this repo's CI on every push
  now.
- `cdlvsm install euglena` → clean error, no panic.
- Unknown package / unknown flag / no-args → clean usage/error, no panic.

**Bootstrap (`install.sh`, own repo):** POSIX `sh`, near-identical to
`code`'s own `install.sh`: Linux x86_64 only, fetches
`codelovesme/cdlvsm-cli`'s latest (or pinned) tag, downloads
`cdlvsm-<tag>-x86_64-linux.tar.gz`, installs `cdlvsm` straight to
`$PREFIX/bin/cdlvsm` (no shim/link distinction needed — `cdlvsm` itself
doesn't collide with anything). Version-pin env var:
**`CDLVSM_CLI_VERSION`** — deliberately distinct from `CDLVSM_CODE_VERSION`
(which `cdlvsm`, once installed, reads at runtime to pin *`code`'s*
version) so the two can't be confused; call out the distinction in both
scripts' header comments.

**CI/release (`.github/workflows/`):**

- `ci.yml`: `cargo build`, `cargo test` on push/PR — no LLVM dependency at
  all, much lighter than `code`'s CI.
- `release.yml`: tag push `v*` + `workflow_dispatch` dry-run (same pattern
  as `code`'s), `cargo build --release`, stage `LICENSE` + `README.md`
  alongside the binary before `tar -czf` (mirroring the SDK/Runtime
  tarballs, not the LICENSE-only `code-lsp` one — `cdlvsm` is a primary
  user-facing binary), a sanity-check step (`dist/cdlvsm --version` and
  `--help` both exit 0) before packaging, upload via
  `softprops/action-gh-release@v2` gated on `github.ref_type == 'tag'`.
  `permissions: contents: write` (easy to forget copying over).
- Repo scaffold checklist: `Cargo.toml`, `LICENSE` (carry forward from
  `code`'s, same license), `.gitignore`, `README.md` (usage examples:
  `cdlvsm install code`, `cdlvsm code run abc.code`, `cdlvsm install
  euglena` documented as not-yet-published, `cdlvsm list`/`uninstall`).

**First ticket in the new repo:** `docs/tickets/done/1-cdlvsm-package-
manager-cli.md`, following this repo's own convention
(`docs/tickets/{done,high,medium,low}/<n>-slug.md`, no T-prefix) since
`cdlvsm-cli`'s closest lineage is `code` (extracted the same way from
`euglena-platform`). Document the dispatch model, the curl/tar-shellout
choice and why, the uninstall-safety property and its test, and explicitly
disambiguate **"cdlvsm the CLI" vs "cdlvsm" as the pre-existing informal
local nickname for the `euglena-platform` checkout** (seen in that repo's
own `dev.sh`/tickets, and even in this repo's own ticket 16, which
references `../cdlvsm/code-vscode` — unrelated, no change needed there,
just worth a sentence so nobody greps "cdlvsm" later and conflates the
two).

No `update` subcommand — re-running `install` already re-fetches
latest/pinned and repoints `current`, which is update. Note this in the
ticket as intentional, not a gap.

### Migration in `codelovesme/code` (this repo)

Full grep-confirmed file list (nothing else references the old script):

- Delete `cdlvsm` (the shell script).
- `install.sh`: update header comment — currently points at itself as
  living in this same repo; point at the new repo/install command instead.
- `README.md`: "Quick install (recommended)" section — replace the
  same-repo `curl .../code/cdlvsm | sh -s -- install code` with the new
  repo's bootstrap (`curl -sSf https://raw.githubusercontent.com/
  codelovesme/cdlvsm-cli/main/install.sh | sh`, then `cdlvsm install
  code`).
- `.github/workflows/pages.yml`: remove **two** spots — the `"cdlvsm"`
  path-filter entry and the `cp cdlvsm dist/cdlvsm` step (the file won't
  exist in this repo anymore).
- `site/downloads.html`: update the cdlvsm install command block to match.
- Update this ticket's status to done and cross-link the new repo once
  built (or split the "done" write-up into a fresh ticket at that point —
  whichever reads more naturally when it actually happens).
- `docs/tickets/done/32-parent-cli-cdlvsm-package-manager.md`: add a
  "Superseded by T33" status line (same phrasing convention T26 already
  uses for T23/T25 — see `docs/tickets/README.md` lines 34/36/37).
- `docs/tickets/README.md`: the Done index table currently stops at T30 —
  T31 and T32 were never backfilled. Add rows for T31, T32 (marked
  superseded), and (once this ticket ships) T33, in the same pass so all
  three land together.

False positives found, no action needed: `.mailmap` line 9 (a git-identity
email alias, coincidental substring), and ticket 16's `../cdlvsm/
code-vscode` references (the pre-existing local-nickname usage, not this
CLI).

## Execution order (for whoever/whichever session picks this up)

1. Switch to the `codelovesme` gh account first — confirm via `gh auth
   status` before any GitHub operation. Confirm `euglena-platform`'s
   visibility for context; create `codelovesme/cdlvsm-cli` as public
   regardless.
2. Scaffold the new repo locally, implement `cdlvsm`, write and run the
   integration tests (including the uninstall-safety regression) locally
   against a temp `$PREFIX`, verify a real install against the live
   `codelovesme/code` release end-to-end (install, `cdlvsm code
   --version` dispatch, list, uninstall) before ever pushing.
3. Push to `main`, verify `ci.yml` green.
4. Tag `v0.1.0`, verify `release.yml` produces a real release asset.
5. Test `install.sh` against that real tag end-to-end from scratch (fresh
   temp `$PREFIX`, real curl of the raw install script) — confirm the full
   loop: bootstrap installs `cdlvsm`, then `cdlvsm install code` /
   `cdlvsm code run <file>` / `cdlvsm list` / `cdlvsm uninstall code` all
   work for real, not just in-repo.
6. Do the `code`-repo migration edits above, verify this repo's CI and
   `pages.yml` deploy still pass after removing the old script.
7. Commit and push both repos (verify `gh auth status` shows `codelovesme`
   active before each push).

## Acceptance criteria

- `codelovesme/cdlvsm-cli` exists, public, with a working `cdlvsm` Rust
  binary supporting `install`/`uninstall`/`list` plus transparent
  passthrough dispatch (`cdlvsm <pkg> <args...>`).
- The uninstall-safety property (never delete a foreign same-named binary)
  has an automated regression test, not just a one-off manual check.
- `install.sh` in the new repo bootstraps `cdlvsm` itself end-to-end from
  a real tagged release.
- The `code` repo's old shell-script `cdlvsm` is removed, with every
  reference (`install.sh`, `README.md`, `pages.yml`, `downloads.html`)
  repointed at the new repo, and this repo's CI/pages deploy still green.
- Ticket bookkeeping caught up: T31/T32/T33 all indexed in
  `docs/tickets/README.md`, T32 marked superseded.

## Effort

Medium-large — new repo with its own CI/release pipeline, a real (if
small) Rust CLI with process-exec semantics to get right, automated tests
that didn't exist before, plus a multi-file migration in this repo.

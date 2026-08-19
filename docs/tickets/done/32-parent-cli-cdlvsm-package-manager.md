# 32 — Parent CLI namespace: `cdlvsm` with installable `code` / `euglena` packages

Status: Implemented and shipped (2026-08-16), then **superseded by
[T33](33-extract-cdlvsm-to-own-repo.md) (2026-08-19)** — the in-repo
POSIX-`sh` `cdlvsm` this ticket delivered was extracted to its own repo
([`codelovesme/cdlvsm-cli`](https://github.com/codelovesme/cdlvsm-cli)) and
rewritten in Rust with dispatch (`cdlvsm code run …`) once it needed to
serve more than one tool. The shell script described below no longer exists
in this repo.

## Scoping decisions made before implementation

The original draft of this ticket was written as a fully speculative
design proposal (see "Original proposal" below) and explicitly framed
itself as if the installer/package-manager layer were a separate
companion repo. Before writing any code, two scope calls were made
(user, via AskUserQuestion):

1. **Build a real `cdlvsm` dispatcher now**, not just an architecture
   doc — `cdlvsm install code` actually works today.
2. **`cdlvsm` lives in this same repo**, alongside the language
   implementation. The "separate companion repo" framing in the
   original draft was a scoping lens for how to think about the
   boundary, not a literal instruction to split repos — doing that for
   a single real package (`code`; `euglena` doesn't exist) would have
   been pure overhead.

## What shipped

- **`cdlvsm`** (new file, repo root, POSIX `sh` — same style as
  `install.sh`): `cdlvsm install <package> [--runtime] [--link]`,
  `cdlvsm list`, `cdlvsm uninstall <package>`.
  - Installs into `$PREFIX/share/cdlvsm/packages/<pkg>/<version>/`, with
    a `current` symlink pointing at the active version.
  - Always creates a `cdlvsm-<pkg>` shim in `$PREFIX/bin`. A bare
    `<pkg>` command (e.g. plain `code`) is **opt-in via `--link`**,
    never the default — `code` collides with VS Code's own `code` CLI
    on Linux, which is exactly the collision risk the original ticket
    raised, so the safe name is what you get without asking for more.
  - `uninstall` only ever removes a shim/link that's a symlink actually
    pointing back into that package's own directory under
    `$PREFIX/share/cdlvsm/packages/<pkg>/` — verified with a real test
    (a plain, non-symlink `code` file standing in for VS Code's binary
    survived `cdlvsm uninstall code` untouched, while `cdlvsm-code` and
    a `--link`ed `code` symlink were correctly removed).
  - `cdlvsm install euglena` fails loudly ("not published yet") instead
    of pretending — there's no such package to install.
  - Verified end-to-end against the real `codelovesme/code` v0.3.0
    GitHub release: SDK tier, Runtime tier (confirmed it correctly
    refuses `build`), version pinning via `CDLVSM_CODE_VERSION`,
    `--link`, `list`, and `uninstall`'s safety check all exercised for
    real, not just read.
- **`install.sh`**: kept fully working, unchanged in behavior — header
  comment updated to describe it as the direct/legacy single-package
  path (plain `code` binary, no shim/package-manager layer), pointing
  to `cdlvsm` for the broader namespace. No functional change, so nothing
  that already depends on `curl install.sh | sh` breaks.
- **`README.md`**: "Installing" section reordered — `cdlvsm` is now the
  recommended quick-install command; `install.sh` is presented after,
  labeled as the no-package-manager direct path.
- **`site/downloads.html`**: same reordering, plus a stray un-migrated
  `fmt` reference (missed by T31's grep because it had no surrounding
  spaces — `<code>fmt</code>`) fixed to `format` here and in
  `.github/workflows/release.yml`'s tier-description comments.
- **`.github/workflows/pages.yml`**: serves `cdlvsm` at
  `https://codelovesme.github.io/code/cdlvsm`, same pattern as
  `install.sh`; added `cdlvsm` to the path-filter trigger list so
  editing it redeploys the site.

## Explicitly not done (deferred, matches the ticket's own scope note)

- **`euglena` package**: doesn't exist as a real artifact anywhere in
  this repo or elsewhere — `cdlvsm install euglena` is wired to fail
  clearly rather than fake support. Nothing to package until `euglena`
  itself exists.
- **Separate GitHub Release assets per cdlvsm-wrapped package**
  (`cdlvsm-code-<version>-...tar.gz` etc.) — `cdlvsm` currently
  re-downloads the *existing* `code-sdk`/`code-runtime` release assets
  unchanged; it doesn't require or produce any new release-artifact
  naming scheme. Revisit if/when there's a second real package.
- **A real package registry / discovery beyond a hardcoded two-package
  case statement** — `cdlvsm`'s `install`/`list`/`uninstall` are
  filesystem-backed and package-list is currently just `code` (real)
  and `euglena` (stubbed-error). No index file, no third-party package
  support. Fine for a two-package (one real) ecosystem; would need
  real design work before a third package shows up.

## Original proposal (for reference — see "Scoping decisions" above for how this was actually interpreted)

The current installer and release artifacts expose a single `code`
binary. That name collides with Visual Studio Code's `code` CLI on
Linux, and it also makes future expansion of related tools harder. If
the ecosystem grows to include more first-party packages such as
`euglena`, LSP helpers, or runtime tooling, a flat `code`-prefixed
model would be confusing and fragile.

Proposed: `cdlvsm` as the parent command/namespace — `cdlvsm code` /
`cdlvsm euglena` for first-party tools, each optionally exposing a
direct binary wrapper (`cdlvsm-code`/`code`), plus package discovery
(`cdlvsm install`/`list`/`uninstall`).

## Acceptance criteria

- ~~The project has a documented architecture for a `cdlvsm` parent CLI
  namespace~~ — superseded: a **working** `cdlvsm` shipped instead of a
  design doc.
- `README.md` and installer docs show `cdlvsm install code` as the
  recommended install path and describe the optional `--link` direct
  wrapper. **Done.**
- ~~Release packaging is designed to produce standalone package assets
  for `cdlvsm`, `cdlvsm code`, and `cdlvsm euglena`~~ — deferred, see
  "Explicitly not done" above; not needed for a one-real-package
  ecosystem.
- `install.sh` or its successor does not assume `code` is the top-level
  installer command by default. **Done** — README leads with `cdlvsm`;
  `install.sh`'s own header now describes itself as the direct/legacy
  path, not "the" installer.

## Effort

Medium — new `sh` dispatcher script with real filesystem-management
logic (versioned installs, safe uninstall that won't touch a foreign
same-named binary), plus docs/site updates. Verified against the live
GitHub release, not just read through.

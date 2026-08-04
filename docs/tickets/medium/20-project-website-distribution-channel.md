# 20 — Project website: Downloads page, hosted install.sh, playground home

- **Priority:** Medium
- **Type:** Distribution (Phase 2 of the distribution roadmap — "docs site")
- **Area:** new website (not yet built), `install.sh` hosting, `README.md`
- **Depends on:** T17 (release artifacts to link to — **T17 done**, `release.yml`
  already produces `code-runtime`/`code-sdk`/`code-lsp` tarballs), T19
  (playground to host)
- **See also:** T22 (language documentation — guide/tutorials/examples/
  reference) is the site's *content*, tracked separately from this ticket's
  site plumbing; T22 depends on this ticket for a site to host it on.

**Status (2026-08-04): live.** https://codelovesme.github.io/code/ —
landing page, Downloads page, and the T19 playground, all deployed via
GitHub Actions (`.github/workflows/pages.yml`) to GitHub Pages.

**Hosting decision:** GitHub Pages, not npm. T19's original design assumed
the site would consume the playground via a published npm package
("dogfooding" the same path a third-party embedder would use) — that's
deferred. The site instead deploys `crates/code-wasm/playground/`'s built
output directly, since GitHub Pages doesn't need an npm intermediate. npm
publishing remains open for third-party embedders, tracked in T19, not
blocking here.

**Repo visibility:** GitHub Pages on the free tier doesn't support private
repositories (confirmed via the Pages API: "Your current plan does not
support GitHub Pages for this repository"). The owner chose to make the
repo public to unblock this — a full commit-history scan for secrets/keys
came back clean beforehand.

**Downloads page correctness:** release asset filenames include the version
(`code-sdk-v0.3.0-x86_64-linux.tar.gz`), so a hardcoded Downloads page would
go stale every release. The page ships with `__VERSION__`/`__SDK_URL__`/
`__RUNTIME_URL__`/`__LSP_URL__` placeholders; the deploy workflow resolves
them from the actual latest GitHub Release at build time (`gh api .../
releases/latest`) and fails the build loudly if any asset can't be found,
rather than shipping a broken Download button. The workflow also re-runs on
`release: published`, so Downloads refreshes right after every future
release without manual edits.

**First real release cut as part of this work:** `v0.2.0` had been tagged
before `release.yml` existed, so the tag-push trigger never fired for it —
only a `workflow_dispatch` dry-run had ever run, no GitHub Release actually
existed. `v0.3.0` is the project's first real release (`code`/`code-lsp`
version-bumped together; `code-native`/`code-abi`/`code-wasm` untouched,
independent publish lifecycles).

Verified beyond "the deploy step succeeded": a real headless-Chromium script
hit the *live* `codelovesme.github.io` URLs directly (not a local
simulation) — playground bindings compute correctly from the deployed wasm,
the Downloads page's SDK link resolves to the real v0.3.0 asset, zero
console/page errors.

## Problem

`curl -sSf https://raw.githubusercontent.com/codelovesme/code/main/install.sh
| sh` works, but doesn't read as a serious project's install story. Real
precedent (rustup, Homebrew, even `dotnet-install.sh`) shows `curl | sh` is a
legitimate advanced/CI-friendly path — but it's never the *leading* story for
those ecosystems. The leading story is a proper install page and/or a native
package manager; `curl | sh` sits behind that, if it's offered at all.

Separately, the browser playground (T19) and the install/download story have
no shared home — there's no project website at all yet.

## Decisions made in discussion (2026-08-01)

- A project website (this ticket, Phase 2) will host: a **Downloads page**
  (platform-specific links straight to the T17 GitHub Release assets —
  `code-runtime-*`, `code-sdk-*`, `code-lsp-*`), the **playground** (T19,
  consumed as the published npm package like any third-party embedder would),
  and **`install.sh` itself**, served from the project's own domain instead
  of `raw.githubusercontent.com`.
- **`install.sh` will eventually be removed as the primary install path** —
  but only once a real replacement is live: this website's Downloads page
  *and* at least one native package manager (open question below). Removing
  it before a replacement exists would be a regression (users falling back to
  manual "download tarball, extract, add to PATH"). This ticket covers hosting
  it professionally and building the Downloads page; it does **not** cover
  removing it — that's a follow-up gated on the package-manager ticket.

## Explicitly deferred — separate, not-yet-decided discussion

- **Which native package manager to launch first** (Homebrew tap, apt/deb,
  AUR, winget, nix, ...). Not decided. Tracked as its own future discussion,
  not blocking this ticket, but blocking `install.sh`'s eventual removal
  (above).
- **Multi-platform binaries** (macOS, Windows) — currently Linux x86_64 only
  per T13's documented scope. Good ideas, explicitly not decided yet; tracked
  together with the package-manager choice since they're related (most
  package managers imply needing the platform binary anyway).

## Proposed change (as implemented)

1. Static site, no framework — plain HTML/CSS, `site/index.html` +
   `site/downloads.html`, matching the playground's own no-build-tooling
   stance:
   - A Downloads page linking to the three current release assets (resolved
     at deploy time from the live latest release, not hardcoded — see
     above), with per-tier guidance (Runtime vs SDK — see T17/T18).
   - `install.sh` served from the site's own domain
     (`https://codelovesme.github.io/code/install.sh`), content copied
     unchanged from the repo root.
   - The playground (T19), deployed from `crates/code-wasm/playground/`'s
     built output directly — not via a published npm package (hosting
     decision above).
2. `README.md`: points the `curl | sh` line at
   `codelovesme.github.io/code/install.sh` (verified resolving), plus links
   to the Downloads page and the playground.

## Acceptance criteria

- [x] Downloads page linking to all three current release assets — verified
  live, resolves to the real v0.3.0 assets.
- [x] `install.sh` reachable from the project's own domain.
- [x] Playground reachable from the same site — verified live via a real
  browser hitting the deployed URL, not just the deploy step succeeding.
- [x] No `install.sh` removal in this ticket.
- [x] `README.md` points at the new domain.

## Effort

Medium, as estimated — **done**, full scope delivered and live.

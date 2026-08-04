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

## Proposed change

1. Static site (framework TBD during implementation) with:
   - A Downloads page linking directly to the three T17 release assets, with
     copy-pasteable per-tier guidance (Runtime vs SDK — see T17/T18).
   - `install.sh` served from the site's own domain (e.g.
     `https://<domain>/install.sh`), content unchanged from T17's version.
   - The playground (T19), embedded via the published npm package — same
     integration path any third party would use, not a special internal
     build.
2. `README.md`: point the `curl | sh` line at the new domain; keep it
   documented as one valid path, not *the* recommended one once the Downloads
   page exists.

## Acceptance criteria

- Downloads page linking to all three current release assets.
- `install.sh` reachable from the project's own domain.
- Playground reachable from the same site, running the real published
  package.
- No `install.sh` removal in this ticket — that's follow-up work gated on the
  package-manager decision above.

## Effort

Medium — mostly a new static site; the individual pieces it links to
(releases, install.sh content, playground) already exist from other tickets.

# Publishing the Code language's VS Code extension

Status: shipping to GitHub Releases + Actions artifacts (this file explains why,
and what flipping to a marketplace requires once its prerequisites hold).
Recorded 2026-08-30; re-read both marketplaces' current offerings before acting.

The extension itself is `editor/vscode/` (thin LSP client; the engine is
`crates/code-lsp` built from the same commit — `code-lsp/Cargo.toml` turns off
the LLVM feature, which is why the pipeline cross-compiles it without a C
linker or a macOS runner). Packaging and shipping live in
[`.github/workflows/publish-editor-vsix.yml`](../../.github/workflows/publish-editor-vsix.yml).

That pipeline deliberately publishes NO to an extensions marketplace today, on
purpose. The checked reasoning is narrower than it looks: neither Microsoft
Marketplace nor Open VSX had a flow (as of writing this), that this repository
could join without keeping a publisher-scoped credential in a place it does not
currently agree to keep one.
Microsoft Marketplace still fronts on a publisher identity plus a personal
access token (a classic secret stored against the repo). Open VSX front-ends on
an account PAT likewise — nothing there offers the "repo owns its own identity,
nothing stored anywhere" shape that the two sibling pipelines settled on:

- `publish-crates-native.yml` — crates.io Trusted Publishing (OIDC exchange,
  zero stored tokens)
- `publish-npm-wasm.yml` — npm Trusted-Publisher (OIDC exchange, zero stored
  tokens)

So until one of those third parties settles into offering a comparable door for
its extension marketplace specifically (both have been moving slowly on this
compared with their language-side flows, though the state changes fast enough
that the check below must be repeated, not remembered), the .vsix rides along
with the CLI: an Actions artifact named `publish-editor-vsix-<version>` plus a
file attached to the very same GitHub Release `release.yml` creates for the tag.
Both of those need only `contents: write` on the workflow token — no new secret,
no publisher account, no per-marketplace configuration, valid on day one and on
every future tag alike — and they guarantee the invariant this project cares most
about: the .vsix a user downloads next to their `code` `<version>` tarball IS the
one built from the exact same tree that produced that tarball.

What flips this to a real marketplace (whichever comes first in practice):

1. One of the two markets adds a repo-bound/OIDC-style publishing path, OR the
   owner decides one long-lived publisher PAT is an acceptable tradeoff for the
   convenience of searchability in both extensions marketplaces. Re-verify what
   each actually offers at the moment you act; do not trust this file's snapshot
   past a month without checking.
2. Register the corresponding publisher, grant it to `codelovesme/code`, store
   whatever credential the flow needs under the repo (or wire the OIDC exchange
   the same way the sibling workflows already are).
3. Swap the workflow's final step (`softprops/action-gh-release`) for the
   appropriate `publish` invocation — and ADD Open VSX alongside Microsoft as
   well, because "searchable" is what half of this exercise exists for, and no
   search box on the GitHub Release page will ever produce that.

Until then this is intentional, documented, reversible packaging; not a stub.

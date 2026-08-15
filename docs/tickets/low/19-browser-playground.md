# 19 — Browser playground: run `.code` in the browser via WASM

- **Priority:** Low
- **Type:** Distribution (Phase 2 of the distribution roadmap — docs site +
  browser playground)
- **Area:** `crates/code-wasm/` (new), `src/module_loader.rs`,
  `src/interpreter.rs`
- **Depends on:** T18 (WASM-capable core — the interpreter/parser/formatter
  must compile to `wasm32-unknown-unknown` with LLVM and native-`.so` off).
  **T18 landed** (`27777de`, CI-verified — the "code Runtime (no LLVM) +
  wasm32 core build" job is green on `main`), so this ticket is unblocked.

**Progress (2026-08-03):**

- Items 1–2 (foundation): the `ModuleResolver` trait (`src/module_loader.rs`,
  `FilesystemResolver` reproduces today's CLI behavior byte-for-byte, verified
  against exact error text) and `Interpreter::bindings()`/
  `Environment::bindings()` (read-only top-level-scope dump, tested).
- Item 3 (`crates/code-wasm` bridge crate): **done for v1 scope** (single
  snippet, no `link`). Exposes `run_source(src: &str) -> JsValue` returning
  `{ ok, bindings, diagnostics }`. Verified with a real, checked-in functional
  smoke test (`crates/code-wasm/smoke-test/run.js`) run under Node against the
  actual compiled `.wasm` — not just a build-only check — wired into CI.

  Two non-obvious toolchain issues found and fixed along the way (see the
  crate README for full detail): (a) a debug build of this crate crashes
  `rust-lld` with SIGSEGV at link time (reproduced identically with a
  separate system `wasm-ld`, so not one linker build's bug) — must always
  build `--release`; (b) `run_source` crashed at **runtime** with `memory
  access out of bounds` (a wasm stack overflow) on essentially any real
  program — the parser's `chumsky` recursion needs ~16MB of stack, which the
  CLI gets via a spawned thread (`src/main.rs`) but wasm32 has no OS threads
  for that trick, so the module's stack is now sized at link time instead
  (`.cargo/config.toml`, `-zstack-size`, scoped to `wasm32-unknown-unknown`
  only). Neither issue was visible at compile time — only found by actually
  running the module, which is why the smoke test (not just a build check)
  is load-bearing and runs in CI on every push.

- Item 4 (minimal web UI): **done for v1.** `crates/code-wasm/playground/` —
  a two-pane source/result workspace (editor + bindings table / located
  diagnostics, rustc-style caret rendering matching `src/diagnostics.rs`), a
  handful of example snippets, light/dark theming. `build.sh` produces the
  browser glue into a gitignored `pkg/` (same not-committed-binaries
  convention as `tests/native_modules/build.sh`). Verified with a real
  headless-Chromium (Playwright) test exercising the actual page — not just
  the underlying WASM API — across three example snippets (constraints,
  handler dispatch, a parse error), confirming zero console/page errors and
  correct rendered output. Also spot-checked visually (light + dark
  screenshots).

  Palette/type choices deliberately grounded in the subject rather than a
  generic dev-tool default — see the design rationale comment at the top of
  `index.html`'s `<style>` block.

**Progress (2026-08-13) — npm packaging:** the explicit go-ahead landed.
Package name confirmed as unscoped `code-wasm` (originally set up as
`@codelovesme/code-wasm`, then switched — a scoped package needs either
an npm org matching the scope or that scope to already be the
publisher's personal username, and unscoped sidesteps that decision
entirely for a first release). `crates/code-wasm/npm/` has a real
`package.json`, a third-party-facing `README.md` (bundler, no-bundler,
and Node usage, each verified — the Node example specifically, since
`--target web`'s default init doesn't work under plain Node; caught by
actually running it, not just writing the doc), and `build.sh` producing
the exact `--target web` glue the playground already uses. Verified with
a full round-trip, not just a build check: `npm pack` → install the real
`.tgz` into a fresh project (not a symlink) → run the documented Node
usage example against it, plus a dedicated `smoke-test.mjs` exercising
resolved bindings, particle construction/dispatch, and a parse error
against the packaged artifact. Wired into CI (`ci.yml`) so this stays
verified on every push, not just today.

**Progress (2026-08-13) — publishing via Trusted Publishing, not a
token:** `.github/workflows/publish-npm-wasm.yml` publishes on a
`code-wasm-v*` tag using npm's OIDC Trusted Publishing — no `NPM_TOKEN`
in GitHub Secrets, nothing long-lived to leak or rotate. Sets
`package.json`'s version from the tag itself, builds, re-runs the same
`npm pack --dry-run` + `smoke-test.mjs` verification against the exact
artifact about to ship, then `npm publish --provenance`.
`workflow_dispatch` runs everything except the actual publish, as a real
dry run (mirrors `release.yml`'s existing dry-run pattern for the CLI).

**Published (2026-08-15): `code-wasm@0.1.0` is live on the npm
registry.** First release was a one-time manual `npm publish` (required
enabling 2FA on the publishing account first — npm now mandates it
registry-wide; the initial attempt failed with a 403 until that was
turned on). Verified for real after publishing, not just assumed: fresh
`npm install code-wasm` into a brand-new project from the actual
registry (not a local tarball), then ran a full particle-construction +
handler-dispatch program through it end to end.

**Trusted Publishing confirmed working end to end (2026-08-15):**
`code-wasm@0.1.1` published via `code-wasm-v0.1.1` tag → CI → OIDC, zero
tokens. `npm view code-wasm` shows the proof directly: `published ...
by GitHub Actions <npm-oidc-no-reply@github.com>` — the maintainer of
record for that release is the OIDC identity itself, not a human login.
Also confirms `--provenance` works correctly (a signed provenance
statement was published to Sigstore's transparency log as part of the
publish).

One real bug found and fixed getting here: the first `workflow_dispatch`
dry run failed — `npm install -g npm@latest` resolved to npm 12.0.2,
which requires a newer Node than the 22.14 pinned (the *documented*
Trusted Publishing minimum, but not what an unpinned `@latest` actually
needs once it moves past that). Fixed by bumping to Node 24 and pinning
npm to major 11 instead of `@latest`, so a future npm major bump can't
silently break this the same way again — caught by actually running the
dry run before trusting it, not by reading the workflow and assuming it
was right.

Since a real trusted publish has now gone through, the last npm-account
hardening step from the original ask is safe to do:
Settings → Publishing access → "Require two-factor authentication and
disallow tokens" — only whoever holds that login can flip it; this
environment has no standing npm credentials and shouldn't be given any
(the entire point of Trusted Publishing is that CI never needs one).
Also still open: wiring
this repo's own `playground/` to consume the *published* package
instead of its own local build (the ticket's "dogfood the public
contract" acceptance criterion — now unblocked, since the package is
live), and wiring the `ModuleResolver`/in-memory source map into
`code-wasm` for `link` support (the trait exists but `run_source`
doesn't use it yet — v1 is single-snippet only, matching the ticket's
"Out of scope (v1)" section). A demo build is
published as a private Claude Artifact (self-contained, wasm embedded
inline, no server) for convenience — this is a throwaway demo, **not** a
substitute for `crates/code-wasm/playground/` (the repo files are the
real, lasting deliverable and what T20's eventual site will actually
use).

## Problem

Trying the language requires installing a binary. There's no zero-install way
to run a `.code` snippet — a browser playground (Phase 2) would let anyone try
the language from a link, and back a docs site's runnable examples.

## Scope decision (2026-08-01): first-class embeddable package, not just an internal playground detail

The WASM build is **not** merely an implementation detail hidden behind our
own docs-site playground page. It's a standalone artifact third parties can
embed in *their own* sites/tools — the same shape as Pyodide (Python-in-
browser, published so anyone can drop it into their own page), not a bespoke
one-off wired only into our playground.

Practical consequence: `run_source`'s JS-facing shape (function signature,
the `{ ok, bindings, diagnostics }` return shape below) becomes a **public
contract** the moment it's published — external embedders will depend on it.
This is much lighter than committing `code_lang`'s whole Rust API to semver
(rejected for `code-lsp`/crates.io in T17 — the language is still under
active structural redesign), because the JS surface here is a small,
deliberately narrow bridge, not the parser/interpreter/AST itself. But it
still means: version this bridge's JS API deliberately, don't let it drift
accidentally alongside internal Rust refactors.

Concretely, this ticket now includes publishing `crates/code-wasm`'s output
as an npm package (name TBD, e.g. `@code-lang/wasm`) with its own README
showing third-party embedding, not just wiring it into our own playground
page. The naming for the wasm artifact itself is intentionally *not* a
`code`/`code-compiler`-style CLI binary name (that pattern is for binaries a
human runs from a terminal) — it's a package name, consumed by a JS host,
same category as any other npm dependency.

## Design decisions on record (from the discussion that produced this ticket)

**Visible output without changing the language.** The language deliberately has
no core I/O — the interpreter never writes to stdout (verified: no
`println!`/`print!` in `interpreter.rs`), and the only core handlers are
`Timestamp`/`Length`, both pure (`interpreter.rs:27-58`). A `.code` program's
observable result is its **final variable bindings** (this is a constraint
language — the program resolves a set of pinned/constrained variables) plus
assertion pass/fail and located diagnostics.

So the playground surfaces the final environment, **not** a print stream. This
is pure host-side observation, not a language change: reading the resolved
bindings adds no syntax, changes no program's meaning, and is invisible to the
program itself (same category as a debugger inspecting variables). Adding a
`Print`/`Log`/console core handler was explicitly rejected — that *would* be
the language's first core I/O and reverse the standing no-core-I/O design
decision. Bonus: a bindings panel also answers the "I made a `Log {...}`, where
did it go?" intuition — the variable simply appears in the panel.

## Proposed change

1. **`ModuleResolver` trait** — extract source-fetching from `module_loader`.
   Today `load()` hardcodes `fs::read_to_string` / `fs::canonicalize`
   (`module_loader.rs:106,119`). Separate "resolve a module ref → (identity,
   source text)" behind a trait; keep cycle detection / SourceMap / parsing in
   the loader. Host implementations:
   - CLI: filesystem (current behavior, moved behind the trait).
   - Playground v1: an in-memory map the JS side populates (single-file or a
     small virtual FS). Keeps everything **synchronous** — no async threading
     through the interpreter.
   - (v2, deferred) remote `link` via URL: the host pre-fetches the whole link
     graph with async `fetch()`, fills the in-memory map, then runs the
     still-synchronous interpreter. URL is a host-layer detail, never baked
     into the sync core. (Bonus: the trait also makes the loader unit-testable
     with in-memory modules instead of temp files.)
2. **Read-only bindings accessor** on `Interpreter` — e.g. `bindings() ->
   Vec<(String, /* value or domain */)>` dumping the top-level scope after
   `execute()`. `env`/`Environment.scopes` are private today with only
   `get`/`get_domain` accessors — add a bulk read. Pure observation, no
   language surface. (Bonus: gives `code run` — which today only prints
   "Program executed successfully." — a way to show what a program produced.)
3. **`crates/code-wasm`** — a `wasm-bindgen` bridge crate depending on
   `code_lang` with `default-features = false` (+ `native-so` off), exposing
   something like `run_source(src: &str) -> JsValue` that returns
   `{ ok, bindings, diagnostics }` (interpreter Result + `error_span` +
   bindings), for JS to render.
4. **Minimal web UI** — editor pane + result pane (bindings table + assertion
   outcomes + located parse/runtime errors). Can live under the Phase 2 docs
   site.

## Out of scope (v1)

- Native module linking (`.so` via `libloading`, `.wasm` via `wasmi`) — no
  filesystem / `dlopen` in browser; single/in-memory source only.
- Remote `link` via URL — deferred to v2 (async pre-fetch, above).
- Any core I/O / `print` — visible output is the bindings panel, not a print
  stream (see design decisions).

## Acceptance criteria

- A `.code` snippet typed in the browser runs with no install and shows its
  final bindings + assertion/diagnostic results.
- No changes to language syntax or semantics — an existing `.code` program
  behaves identically; only its result is now displayed.
- `code_lang` still builds and behaves identically for the CLI (the resolver
  trait and bindings accessor are additive).
- `code-wasm`'s build is published as its own npm package, with a README
  demonstrating embedding it in a page that isn't our own docs site.
- Our own docs-site playground consumes the published package like any other
  third-party embedder would (dogfooding the public contract, not a special
  internal-only build path).

## Effort

Medium — the resolver-trait extraction and the new `code-wasm` crate + UI are
the bulk; the bindings accessor is small; npm packaging/publishing is a small
addition on top. Gated on T18.

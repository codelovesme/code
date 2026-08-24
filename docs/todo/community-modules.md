# Community modules

Distribution story for modules: what ships inside the `code` binary, what
we publish ourselves (per host — native artifacts and browser npm
packages), and how third parties publish theirs. Direction decided with
the owner 2026-08-23; the phases below are the implementation order.

## Tiers — decided

Three tiers, distinguished by *where the bytes live*:

| tier | what | how users get it |
|---|---|---|
| **core** | `Length` (shipped), `Timestamp` | compiled into the `code` binary — nothing to install, works everywhere including the wasm playground |
| **first-party modules** | `terminal`, `console`, `math`, `strings`, `net`, … | native: GitHub Releases + `code install <name>`; browser: npm |
| **community modules** | anyone's | the author publishes to *their own* GitHub Releases (a template repo provides the CI); consumers install by URL first, by name once an index exists |

The rule separating tier 1 from tier 2: **fundamentals are core**. Only
what the language itself needs to stay meaningful (`Length`, `Timestamp`)
is compiled in. Everything else is a module, and modules are things you
install. Printing leads batch 1 as two host-specific modules — `terminal`
(native) and `console` (browser) — deliberately *not* baked into the
binary, because keeping the binary free of modules keeps "what ships
inside `code`" a closed list.

### Hosts: one module per host — decided

Each host's linker speaks exactly one dialect: native `link` loads a
`.so`/`.a` from disk, while code-wasm's resolver maps a `link` alias onto
a JS object handed to `run_with_modules` (see `PreloadedModules` in
`crates/code-wasm/src/lib.rs`). There is no artifact that serves both,
so modules are per-host: same idea, different name, different artifact.
This keeps every capability story exact — a module either has the
particle or it doesn't, no "supported except in the browser" footnotes.

Naming: the browser-side twin takes the familiar web name (`console`),
the native side the plain one (`terminal`) — decided 2026-08-23. Where
two hosts implement the same capability they share the particle's name
and result shape (`Print`, `Warn`, `Error`, `Debug`); host-only particles
make no apology (`Read` in `terminal`, `Group`/`GroupEnd` in `console`).

Packaging follows the host: native modules ship as GitHub Release assets
plus `code install`; browser modules ship as npm packages exporting a
`run_with_modules`-shaped object, version-pinned by the site/playground
exactly like `code-wasm` already is.

### Core addition (phase A)

- `Timestamp` — `emit { "_class": "Timestamp" } to core` →
  `{ "_class": "TimestampResult", "value": <unix seconds> }`. Revived from
  the old language, which had exactly this.

Lands in `src/interpreter.rs::dispatch_core` **and**
`src/runtime.c::code_core_dispatch` with identical operand rules and error
wording (the standing two-backends parity invariant), plus fixtures run
under both `code run` and `code build`.

## Distribution (phases B–D)

### Hosting: GitHub Releases

No registry server. Provenance is free (source and CI are visible per
artifact), and it is where the community lives anyway. Per release:

- one asset per platform: `math-linux-x86_64.so`, … (the extension follows
  the platform; the loader already keys off suffix)
- one `module.json`: name, version, ABI version, handler list, exported
  vars, supported platforms, sha256 per asset
- everything built from source in CI (cross-compilation matrix), never
  uploaded by hand

Matrix today: linux-x86_64 only. Deferred, each one job block away once
demand shows up: linux-arm64 (cross gcc), macos-{x86_64,arm64}
(`-dynamiclib` + ad-hoc codesign — unsigned dylibs get killed/quarantined),
and **Windows**, which additionally needs a `LoadLibrary`/`GetProcAddress`
path in `src/native.rs` and `runtime.c` — real work, tracked as phase F.
Build on an older-glibc runner image for maximum compatibility.

### Install: `code install` — shipped 2026-08-23

Subcommands: `code install <name-or-url> [--global]`, `code remove <name>`
(alias `rm`), `code ls` (installed + available).

- Resolution: first-party names hit an index (which starts life as a JSON
  file in this repo, served from the Pages site); community modules install
  by full URL until a name-based community index exists (phase F).
  Shipped form: the index maps name → latest release-tag page, and the
  manifest URL is derived from it (`…/releases/tag/TAG` →
  `…/releases/download/TAG/<name>.json`); a by-URL reference is the
  manifest URL itself. The default index URL is overridable via
  `CODE_MODULE_INDEX`.
- Storage: project-local `.code/modules/<name>/<version>/` plus
  `.code/lock.json` recording name, version, source URL, and sha256;
  `--global` installs to `~/.code/modules/` but records the same project
  lockfile (with a `global` flag), so there is exactly one lockfile per
  project.
- Verification: sha256 checked at download time **and** re-checked at load
  time while a lock entry exists (modules are small, so this is cheap — a
  tampered or replaced `.so` fails loudly instead of loading). Enforcement
  is scoped to bytes sitting under an install root, so vendored copies
  elsewhere stay unpinned.
- Trust model for v1: you can inspect exactly what loads — the lockfile
  pins a hash, and the hash points at a public artifact whose source and CI
  are visible. Artifact signing (minisign) is a later phase, not day one.

### Loader: fallback chain — shipped 2026-08-23

`FilesystemResolver` used to resolve `link "x.so"` against the linking
script's directory only — deliberately no search path, so where a module
comes from is always answerable by looking at the two files involved. The
agreed shape is now implemented:

1. the script's directory (unchanged — explicit wins)
2. the nearest ancestor's `.code/modules/` (walk up, like `node_modules`)
3. `$CODE_MODULE_PATH` (colon-separated, for unusual setups)
4. `~/.code/modules/` (globally installed)

plus a fifth lookup that makes installed layouts reachable: a bare filename
maps through the project lockfile onto `<root>/<name>/<version>/<asset>`
(the layout `code install` writes). Without the lockfile entry the layout
lookup has nothing to go on — the lockfile is the single source of truth
for "what is installed here", and flat vendored layouts keep resolving
through steps 1–4 unchanged. Documented wherever `link` is documented. The
provenance guarantee became: answerable by looking at the script, its
lockfile, and four fixed places.

## First-party modules (phases B, D)

Native modules live in this repo under `crates/modules/<name>/`, each a
tiny `cdylib` on `code-native`, sharing one CI workflow that builds the
platform matrix and publishes to GitHub Releases. Browser modules are npm
packages exporting a `run_with_modules`-shaped object (see "Hosts"),
published alongside. Every module ships `tests/*.code` fixtures run under
both output modes in CI — dogfooding the parity invariant and giving
consumers confidence.

Batch 1 (proves the pipeline):

- **terminal** — the native host's console: `Print` (stdout), `Warn` /
  `Error` (stderr, ANSI color when attached to a TTY), `Debug` (silent
  unless `CODE_DEBUG=1`), `Read` (interactive stdin prompt). Being a plain
  `.so`, `Print` just writes stdout in C — no purity seam needed in the
  interpreter.
- **console** — the browser host's console, an npm package: `Print` →
  `console.log` (DevTools pretty-prints our JSON-shaped values as
  expandable trees), `Warn` / `Error` / `Debug` → their `console.*`
  counterparts, `Group` / `GroupEnd` → `console.group`. No `Read`: there
  is no stdin in a browser, and the module doesn't pretend otherwise.
  Overlapping particles share names and result shapes with `terminal`'s.
- **math** — shipped 2026-08-24: `Double`, `Sum`. Written in Rust on
  `code-native`, inheriting the numeric half of `test_math` under the split
  proposal (`Shout`/`Echo` went to **strings**; `test_math` stays a pure
  test double with all four, so the split cost it nothing). Design
  decisions live in the module's header comment (plain `f64` end to end —
  no rounding/formatting policy invented; `Sum` over an empty array is 0;
  non-number operands refused rather than coerced).
- **strings** — shipped 2026-08-23: `Shout`, `Echo`, `Split`, `Join`,
  `Trim`, `Upper`, `Lower`. Written in Rust on `code-native` — the first
  first-party module to take the Rust path. Rationale: `terminal` stays C
  because it is the canonical reference implementation (zero framework
  between a reader and the ABI), while everything whose substance is logic
  rather than syscalls goes through the crate; that split keeps both paths
  exercised in production, so an ABI drift fails differently in a C module
  and a Rust module — exactly the diagnostic you want. Design decisions
  live in the module's header comment (ASCII-only case conversion, empty
  segments survive `Split`, `Join` refuses non-string elements, multi-char
  separators refused outright).

Batch 2 (flagship — proves the pipeline with a non-trivial module):

- **net** — minimal blocking HTTP: `Get`/`Post` particles taking a url plus
  optional headers, returning `{ status, headers, body }`. Plain blocking
  sockets via libc, no async dependency.

Later candidates, only when asked for: rand, date/time formatting (raw
seconds are core `Timestamp`; human-readable formats belong in a module),
fs, json parse/stringify.

## Community path (phase E)

- A `code-module-template` repo: CI workflow (matrix + releases), fixture
  harness skeleton, README skeleton, and a prominent license notice.
- License reality, stated upfront: every native module embeds `runtime.c`
  (that is how the ABI's value-lifetime contract works), so modules are
  GPL-3.0 derivatives. Fine for most contributors; the template says so in
  bold.
- Publish flow: fork the template → implement → tag → CI publishes to your
  releases → share the URL. Nothing central to maintain until the index.

## Website (phase E)

A **Modules** page beside the Packages section: renders each module's
`module.json` (handlers, versions, platforms, install command), sourced from
the same data `code ls` reads. It doubles as the de-facto index.

## Phases

| # | deliverable | unblocks |
|---|---|---|
| A | core `Timestamp` (both backends, fixtures) | the core-handler pattern proven before any module ships |
| B | `crates/modules/{terminal,math,strings}` + cross-build CI → GitHub Releases, `console` npm package; dogfood by hand | proof the pipeline works |
| C | ~~`code install/remove/ls` + resolver fallback chain + lockfile/sha256~~ — shipped 2026-08-23 | users getting modules without copying files |
| D | `net` module | the flagship community-facing module |
| E | template repo + publish guide + website Modules page | other people publishing |
| F | (optional) Windows `.dll` support, name-based community index, artifact signing | wider reach / stronger trust |

## Still open

- `Print` operand policy (both `terminal` and `console`): strings only, or coerce numbers/booleans too?
- ~~Walk-up semantics for `.code/modules/`~~ — decided 2026-08-23 with the
  fallback chain: stop at the nearest `.code/` directory (shared helper
  `find_project_code_dir` in `src/loader.rs`); git-root detection deferred
  until it earns its complexity.
- `net` API shape: one particle per verb (`Get`/`Post`) vs one `Request`
  particle carrying a method field — sketch in the module's README before
  coding.

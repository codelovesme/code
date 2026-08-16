# 22 — Language documentation site: guide, tutorials, examples, reference

- **Priority:** Medium
- **Type:** Distribution (Phase 2 of the distribution roadmap — docs
  *content*, distinct from T20's site plumbing)
- **Area:** New content, hosted on the T20 project website (framework
  decided there); no new infrastructure of its own.
- **Depends on:** T20 (project website — needs a site to host this content
  on; this ticket is pure content + its integration into that site's
  structure, not a new site).

## Problem

There is currently no learning path for someone encountering the language.
`README.md` covers the basics, but there's no structured tutorial, no
worked examples beyond the test suite, and no explanation — written for a
newcomer, not a contributor — of the language's genuinely distinctive
paradigm choices: constraint-based variable semantics (`a = 5`, `a > 5`,
`a ∈ Z` narrow a domain, they don't "assign" in the imperative sense),
**no user-defined functions** (handlers/particle dispatch are the only unit
of reusable logic — see [[code-language-design-decisions]]), single
assignment, and **no core I/O** (a program's visible result is its final
top-level bindings, not printed output — directly why the T19 playground's
bindings panel exists). A newcomer coming from an imperative or functional
language has to read source and test fixtures to piece this together today.

## Scope note: content, not infrastructure

T20 owns the site itself (framework, hosting, Downloads/install/playground
pages). This ticket owns everything a reader actually reads: the guide,
tutorials, examples, and reference — delivered as content that gets dropped
into T20's site structure once it exists. Don't duplicate T20's plumbing
work here.

## Proposed content, phased (each phase independently valuable)

**Phase 1 — Getting Started + core Language Guide.** The highest-value
slice on its own: takes a newcomer from zero to running their first program
(via the T19 playground — no install needed) and explains the concepts that
actually differ from what they already know:
- Constraint variables & domains (equality, ranges, `in`/type constraints,
  narrowing, freezing).
- Single-assignment (why, and what it rules out — `if` in this language
  narrows rather than branches assignment).
- Handlers & particles: the no-functions model (`emit X to target get
  result`), why the language doesn't have `name(args)` call syntax (see
  [[code-language-design-decisions]] and ticket 11/12's retirement of
  `Expression::Call`).
- Types & constraints: particle type declarations, union/intersection
  types, optional fields.
- Loops: `loop var over <array>` (bounded iteration only — no
  `while`/counters/recursion, and why that's a design choice, not a gap).
- Modules: `link`, public/private visibility, module aliasing.
- **No core I/O**: a program's result is its final bindings, not printed
  output — the paradigm shift a reader needs before the playground's
  bindings panel makes sense.

**Phase 2 — Step-by-step tutorials.** A few worked examples building
something small end-to-end, not just syntax snippets. `tests/euglena/`
(`main.code` + `organelles/`/`particles/` submodules, exercised by
`tests/euglena_*.code`) is already a realistic multi-module program using
handlers, particles, and module linking together — worth adapting into a
guided walkthrough instead of authoring a tutorial's example code from
scratch.

**Phase 3 — Examples gallery.** Runnable snippets demonstrating common
patterns, each embeddable via the published `code-wasm` package (T19) so a
reader can edit and re-run them in place, not just read static code blocks.

**Phase 4 — Reference.** Full constraint/type syntax reference, the
built-in core handlers (`Timestamp`, `Length` — see T12), and the CLI
(`code run`/`build`/`format`/`test`).

## Non-negotiable: every example must actually run, verified in CI

This project's whole engineering culture this far (98/98 leak-free
verification, the code-wasm smoke test that caught two real toolchain bugs
invisible at compile time, etc.) has been "verify, don't assume." Docs
examples are exactly where language drift silently rots content: a syntax
change lands, nobody notices the tutorial now shows something that no
longer parses. **Every runnable example in this site must be a real
`.code` file (in `tests/` or a dedicated docs-examples directory) checked by
`code test`-equivalent CI, not prose-embedded code blocks that are never
executed.** This is the same discipline already applied to every other
piece of example code in this repository — docs content doesn't get an
exemption.

## Out of scope

- Site framework, hosting, Downloads/install/playground pages — T20.
- Publishing `code-wasm` to npm — T19 (needed for Phase 3's embedded,
  editable examples; Phases 1–2 don't require it).

## Acceptance criteria

- Phase 1: a newcomer unfamiliar with constraint languages can go from the
  Getting Started page to running their first program and understanding
  *why* the language has no functions and no I/O, without reading source.
- Phase 2: at least one full worked tutorial building something end-to-end
  across multiple modules/handlers.
- Phase 3: examples gallery live and editable in-place via the playground.
- Phase 4: reference section covers the full constraint/type syntax, core
  handlers, and CLI.
- Every example on the site is a real, CI-verified `.code` file — zero
  prose-only code blocks that could silently go stale.

## Effort

Large — real content-authoring work, the biggest ticket in the backlog by
writing effort, not engineering complexity. Ship phase by phase; Phase 1
alone is a substantial improvement over the status quo (nothing).

## Progress

**Phase 1 — Getting Started + core Language Guide: implemented (2026-08-05).**
- `site/guide.html` — a single-page guide with a sticky section TOC,
  matching the existing site design system (teal accent, warm neutrals,
  light/dark). Eight sections: getting started, constraint variables,
  single assignment, types, handlers (no functions), loops, modules, no
  core I/O. Nav link added to `index.html` and `downloads.html`.
- **The non-negotiable ("every example runs, verified in CI") is satisfied
  structurally, not by promise.** The eight concepts are backed by eight
  real programs in `docs/examples/*.code` (plus one linked module,
  `modules/handler-module.code`). `docs/examples/run.sh` executes every one
  via `code run` and is wired into CI (`ci.yml`, "Documentation examples run
  clean" step). The guide's code blocks are **not** hand-copied — the
  template ships `__EXAMPLE__<slug>__` placeholders and
  `site/inject-examples.py` substitutes the exact (HTML-escaped) file
  contents at Pages-assembly time (same pattern `downloads.html` uses for
  version strings). The script fails the deploy on any missing file or
  unfilled placeholder, so the published page literally cannot show a
  snippet that differs from what CI ran. Verified end-to-end locally: all 8
  examples pass, injection fills all 9 blocks, and the rendered page was
  checked in real headless Chromium (light + dark, no console errors, no
  horizontal overflow, all blocks non-empty).

**Phase 2 — Step-by-step tutorial: implemented (2026-08-05).**
- `site/tutorial.html` — a 4-step walkthrough (type → handler →
  module split → batch via `loop`) building one thing end-to-end (an
  order-validation pipeline), narrated separately from the Guide's
  per-concept structure. Same design system, same nav.
- Same non-negotiable, same mechanism: `docs/examples/tutorial/*.code`
  are real files run by `docs/examples/run.sh` in CI;
  `site/inject-examples.py` extended with a `tutorial_` slug prefix so
  the tutorial page's code blocks are injected the same way the guide's
  are — can't drift from what CI runs.
- Nav link added across all site pages.

**Phase 4 — Reference: implemented (2026-08-09).**
- `site/reference.html` — full syntax reference by category, not
  narrative: values & literals, constraint operators, domains,
  set operators, particle types, handlers & dispatch, loops, modules,
  core handlers, and the CLI (`run`/`build`/`format`/`test`). Same design
  system, same nav, new compact `table.forms`/`.cli-cmd` components for
  quick-lookup syntax tables and command listings.
- Same non-negotiable, same mechanism: 10 real files under
  `docs/examples/reference/*.code`, run by `docs/examples/run.sh`
  (extended for the directory) in CI; `site/inject-examples.py`
  extended with a `reference_` slug prefix. The CLI section is
  hand-written prose (shell usage, not `.code` examples) — verified
  manually against `code`'s actual `--help` output and `main.rs`'s
  argument handling while writing it (notably: `code test` takes no
  path argument, always scans `tests/` relative to the cwd — a detail
  easy to get wrong by copying the pattern used everywhere else in this
  session, `code test tests/`, which silently ignores the extra arg).
- Nav link added across all site pages.

Phase 3 (examples gallery) remains open — needs T19 (npm-published
`code-wasm`) first, not yet done.

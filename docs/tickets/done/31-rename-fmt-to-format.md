# 31 — Rename `code fmt` command to `code format`

Status: Implemented and shipped (2026-08-16).

## The decision

`code fmt` was renamed outright to `code format`, with **no `fmt` alias
kept** — a deliberate deviation from this ticket's original proposal
(which suggested keeping `fmt` as a backward-compatible alias). The
project pushes straight to `main` with no release-consumer base yet to
break, so there was nothing to preserve compatibility for; carrying a
second name for the same subcommand indefinitely would have been dead
weight, not a kindness.

## What changed

- `src/main.rs`: the `"fmt"` match arm, its usage string, and the
  `fmt_file`/`fmt_one` helper functions all renamed to `"format"`/
  `format_file`/`format_one`. Help text and the `--check` failure
  message's suggested command both say `code format` now.
- `src/format.rs`: doc comment reference updated (`code fmt` →
  `code format`); the formatting engine itself (`format_document`) was
  never named after the CLI verb, so it's unchanged.
- `tests/fmt_cli.rs` renamed to `tests/format_cli.rs`; every test name,
  temp-file prefix, and CLI arg updated to `format`.
- `README.md`, `.github/workflows/ci.yml`, `site/reference.html`: every
  `code fmt` reference updated to `code format`.
- Still-open tickets that mention the CLI surface in passing
  (`17-split-release-artifact-code-lsp.md`,
  `18-wasm-capable-core.md`, `22-language-documentation-site.md`)
  updated for consistency. Closed tickets that predate this rename were
  left as-is — they're a historical record of what was true when
  written, not living documentation.

## Acceptance criteria

- `code format` invokes the formatter. **Done.**
- ~~`code fmt` still works as an alias~~ — decided against; no alias.
- Documentation and examples updated to reference `code format`. **Done.**
- Tests cover the new command name. **Done** (`tests/format_cli.rs`).

## Effort

Small — command rename plus docs/test updates, as scoped. No alias
plumbing needed since none was built.

# T9 — Carry source spans on the AST for located runtime/codegen errors

- **Priority:** Medium (interpreter slice done; codegen + multi-file deferred)
- **Type:** Architecture / diagnostics
- **Area:** `parser.rs`, `ast.rs`, `interpreter.rs`, `codegen.rs`, `diagnostics.rs`

## Status / what shipped

**Done (interpreter + codegen, single-file):**
- `ast::Spanned<T>` + `ast::Span`; all statement lists are now
  `Vec<Spanned<Statement>>`. The parser tags each statement via
  `map_with_span`. `interpreter`, `codegen`, `environment`, and `module_loader`
  were adapted mechanically (per-statement functions unchanged; only the
  iterating callers unwrap `.node`) — no behavior change from the plumbing.
- Both backends track the executing/compiling statement's span: the interpreter
  exposes `error_span()`, and codegen returns a `CodegenError { message, span }`.
- `code run` **and** `code build` render located errors rustc-style
  (`file:line:col` + caret) via `code_lang::diagnostics`, sharing one `Diag`
  reporter in `main.rs`.

**Done (multi-file provenance):**
- `module_loader` now owns a `SourceMap`: each loaded file is assigned a global
  char-offset base, and `shift_spans` rebases that file's statement spans into
  the shared space. A span therefore self-identifies its file, so a runtime or
  codegen error in a linked module renders against *that* module's source — no
  single-file restriction, and no `Spanned`/interpreter/codegen type changes
  (spans stay `Range<usize>`, now global). `load_program_with_links` returns
  `(Program, SourceMap)`; `main.rs`'s `Diag` resolves offsets through it.

**Deferred (still open):**
- **Expression-level granularity:** errors point at the enclosing statement, not
  the exact sub-expression. This is the last remaining piece and needs spans on
  `Expression` nodes (parser + both backends' expression handling).

## Context

Parse errors now render with `file:line:col` + a source caret via
`code_lang::diagnostics` (see the "rustc-style diagnostics" work). Runtime and
codegen errors, however, are bare `Result<_, String>` messages with **no source
location** — because the AST nodes carry no span information.

## Problem

The parser produces a plain AST (`ast::Expression` / `ast::Statement`) with no
byte/char offsets, so by the time the interpreter or LLVM backend raises an
error it cannot say *where* in the source it happened. For small single files
this is tolerable; for larger, `link`-ed multi-file programs it means hunting by
hand — worst for anonymous errors like `contradictory constraints`,
`arithmetic type mismatch`, `division by zero`.

## Proposed change

Thread spans end-to-end:

1. Attach a span (char range, matching `chumsky`) to AST nodes — at least to
   `Expression` and `Statement`, e.g. a `Spanned<T>` wrapper or a `span` field.
2. Have the parser populate them (chumsky `map_with_span`).
3. Change interpreter/codegen error types from `String` to an error that carries
   an optional span (or thread the current node's span through), and render via
   `code_lang::diagnostics::render` — the renderer already exists and is reused.

This is a large, cross-cutting refactor of a currently green, well-tested
codebase, so stage it: interpreter first (biggest win), codegen after.

## Interim mitigation (done separately — "work 2")

Runtime/codegen messages were enriched to include the actual type/identifier
(e.g. `expected Number, found String`) so anonymous errors became more
self-locating without spans. That reduces — but does not remove — the need for
this ticket.

## Acceptance criteria

- A runtime error (e.g. arithmetic type mismatch) prints `file:line:col` + a
  caret, matching the parse-error style.
- The `.code` and cargo suites stay green.

## Effort

Large. The main risk is regressions from touching the parser + AST + both
backends at once; mitigate with incremental, per-layer PRs.

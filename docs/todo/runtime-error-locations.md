# Runtime errors have no source location

> **Option 1 shipped 2026-08-27**, under `code run`. A runtime error now
> points at the top-level statement it came from; a failure nested in an
> `if`/`loop` body reports that enclosing statement. `code build` is
> unchanged and still prints a bare message — the "Note on the compiled
> backend" below is the only part of this document still open.

Parse and lex errors point at the offending line and column
(`src/span.rs`, shipped 2026-08-23):

```
error: expected an expression, found Newline
 --> demo.code:2:12
  |
2 | let b = a +
  |            ^
```

Runtime errors still don't:

```
error: assertion failed
error: undefined variable 'x'
```

`assert` is the one that hurts. Every other runtime error names the thing
that went wrong — `undefined variable 'x'` is greppable, a type error says
which operator — but `assertion failed` says nothing at all, and the fixture
suite alone has 127 files that are mostly asserts.

## Why it was left

Locating a runtime error means knowing which statement was executing, which
means carrying a span on AST nodes. That is a much more invasive change than
the parse-error one, and invasive in the specific way the owner asked this
feature *not* to be (2026-08-23: "lightweight and easily alterable, not heavy
and dependent everywhere").

The parse-side change avoided it entirely: the position is attached at
exactly one place (`parser::parse`, from wherever the parser stopped), so
none of the ~24 individual error sites know spans exist, and neither does
the AST, the interpreter, or codegen. `span::Located` is created by
`lexer`/`parser` and dies in `loader`, which is the only place holding both
a module's text and its name. Three files, no ripple.

None of that helps at runtime, because by then the tokens are gone.

## Fix direction

Two options, in increasing order of cost and precision:

1. **Top-level statements only.** `Program` gains a `starts: Vec<u32>`
   parallel to `statements`. The interpreter's top-level loop already
   walks those with an index, so it can record the current one and decorate
   any `Err` on the way out. Exact for a top-level `assert` (which is most
   of them); a failure nested inside an `if`/`loop` body reports the
   enclosing top-level statement instead. **The AST itself stays
   untouched**, and codegen ignores the new field entirely.

2. **Spans on every statement.** `Vec<Stmt>` becomes `Vec<Spanned<Stmt>>`
   throughout. Exact everywhere, at the cost of ~20 mechanical touch points
   across `parser.rs`, `loader.rs`, `interpreter.rs`, and `codegen.rs` —
   including `verify_stmts`, `gen_if`, `gen_block`, `gen_loop`, and
   `gen_import`, none of which otherwise care where a statement came from.

Option 1 is the one to reach for first: it buys the `assert` case, which is
the whole point, for roughly a tenth of the disruption. Worth confirming
against real use that enclosing-statement attribution is actually too coarse
before paying for option 2.

## What shipped

Option 1, close to as sketched. The one addition the sketch did not
anticipate: `starts` alone is not enough to *render* anything. By the time a
runtime error happens the loader has dropped the source text, and
`interpreter::run(&program)` has nothing else to work from. So `Program`
gained two fields rather than one:

- `starts: Vec<u32>` — char offset of each top-level statement, filled by
  `parser::program()` before it parses each one, so it is the offset of the
  statement's *first* token.
- `origin: Option<span::Origin>` — the entry module's display name and text,
  attached by `loader::load`. It is `None` for a module and for any
  hand-built `Program`; `Program` derives `Default` so those write
  `Program { statements, ..Default::default() }` and opt out entirely.

Only the *entry* module gets an origin, and that falls out of the existing
design rather than being a limitation to work around: a linked module's
statements are folded into a `Stmt::Import` body by `resolve_link`, so they
are no longer top-level statements at all. A failure inside a linked module
therefore reports the entry file's `link` line — the same enclosing-statement
rule as `if`/`loop`, applied one level up.

Rendering is one site: the top-level loop in `run_with` decorates any `Err`
via a small `locate` helper, mirroring how `parser::parse` attaches a
position once for all ~24 parse error sites. **No AST node carries a span**,
no individual error site knows offsets exist, and codegen ignores both new
fields — which is what keeps this "lightweight and easily alterable" rather
than heavy and depended on everywhere.

### Coverage

`tests/error_locations.rs` — the file that previously pinned
*`runtime_errors_are_not_located_yet`*, which is why closing this gap had to
come here on purpose. It now covers the exact rendering of a failing
top-level `assert`, that other runtime errors (`undefined variable`) are
located too, that a nested failure reports its enclosing statement, and that
an origin-less `Program` still produces a bare message.

The fixture harness needed no changes and got none: it only ever checks
pass/fail, never message text (`run_language_tests.rs`), so nothing there was
silently relaxed.

## Note on the compiled backend — still open

`code build` still needs its own answer. A compiled
binary's runtime errors are raised by `runtime.c`
(`code_runtime_error`), which has no idea what line it came from — passing
one in would mean threading a location through every call site in the
generated IR. So the two output modes now differ in how well they
*report* an error, though not in which programs error — the standing
run/build invariant is about behaviour, not message text, and the fixture
harness only checks that both modes agree on pass/fail. That divergence is
stated in the README's "two output modes" section rather than left to be
discovered.

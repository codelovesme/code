# Runtime errors have no source location

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

## Note on the compiled backend

Whatever is done here, `code build` needs its own answer. A compiled
binary's runtime errors are raised by `runtime.c`
(`code_runtime_error`), which has no idea what line it came from — passing
one in would mean threading a location through every call site in the
generated IR. Until then the two output modes will differ in how well they
*report* an error, though not in which programs error — the standing
run/build invariant is about behaviour, not message text, and the fixture
harness only checks that both modes agree on pass/fail.

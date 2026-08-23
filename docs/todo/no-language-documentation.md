# The new language has no documentation

There is no `README.md` at the repo root. The only prose describing a
language called `code` is `old/README.md`, which documents the *archived*
language — constraints, particles, handlers, `∈`, `≤`, `emit ... to core` —
essentially none of which exists any more. Anyone landing on the repo reads
the wrong language.

What exists instead, and is accurate:

- `tests/*.code` — every feature, executable, and proven correct in both
  output modes by `tests/run_language_tests.rs`
- doc comments in `src/ast.rs` — the decisions and their reasons, per
  construct
- `src/runtime.c`'s header comments — the compiled value model and
  refcounting rules

So the material is there; it is just not addressed to a reader.

## What it should cover

The language as it now stands: `let` and reassignment, the six JSON value
kinds, `.field`/`[index]` (read-only, null on invalid access), operators and
their operand-type rules, `assert`, `if` (no `else`, ever), bare blocks,
every `loop` form (`over` with optional index, bare `loop { }`, and the
`get name [= init]` accumulator), `break`, and `continue`.

Worth stating explicitly, because each is a deliberate decision a reader will
otherwise read as an omission: no functions, no `else`, no `while` (`loop { }`
is how an unbounded loop is written), no mutation of a constructed value, no
type keywords.

Also worth a short section on the two output modes (`code run` / `code build`)
and the rule that binds them — every feature must behave identically in both,
which is what the fixture harness enforces.

## Note

`old/README.md` should probably be left exactly as it is and the root README
should say plainly that `old/` is an archive, so the two are never mistaken
for each other.

# T10 [DONE] — Negative number literals don't parse

- **Priority:** Medium
- **Type:** Parser gap
- **Area:** `src/parser.rs`, `src/ast.rs`

## Problem

`-5` is not a valid number literal. Unary minus does not exist in the grammar —
only binary subtraction (`a - b`) is parsed. `UnaryOp` (`src/ast.rs`) has a
single variant, `Not`; there is no `Negate`.

### Evidence

```
x = -5
```

```
error: Unexpected: x = -5
  --> x.code:1:1
  |
1 | x = -5
  | ^^^^^^
```

A negative value can currently only be produced by subtracting from an
existing value (`0 - 5`), which itself requires a variable/literal to subtract
from — there is no way to write a negative literal directly, including as a
function/handler-field default, array element, or assert comparison.

## Proposed change

Add unary minus to the grammar and AST:
- `UnaryOp::Negate` (or fold directly into number-literal parsing, i.e. accept
  an optional leading `-` immediately before a numeric literal with no
  whitespace, matching common "negative literal" conventions and avoiding
  ambiguity with binary subtraction).
- Implement `Negate` in the interpreter (`eval_expr` for `Expression::Unary`)
  and in codegen (LLVM lowering, mirroring how `Not` is handled in both
  backends).
- Decide precedence: should `-5 * 2` parse as `(-5) * 2` (typical) — pin this
  down and add a precedence-table test alongside the existing operator tests.

## Acceptance criteria

- `x = -5` parses and `assert x = -5` (well, `assert x ≠ 1` etc.) passes under
  both the interpreter and the LLVM backend (`code build --target exe`).
- `-5 * 2 = -10` (precedence test).
- Existing subtraction behavior (`a - b`) is unchanged.

## Effort

Small–Medium: grammar + one AST variant + two backends (interpreter, codegen),
each mirroring existing `Not` handling.

## Resolution (implemented)

Went with the general prefix-operator approach (not the literal-only
alternative floated above) — `UnaryOp::Negate` sits at the exact same grammar
tier as `UnaryOp::Not`, both folded by the same `unary` parser rule. This is
also **not whitespace-restricted** (`- 5` parses the same as `-5`): a
precedence-climbing grammar has no actual ambiguity to guard against here —
`additive`'s loop already consumes a binary `-` before invoking the next level
down for the right operand, so a *second* leading `-` there (e.g. `a - -5`)
unambiguously means "unary-negate the operand," which is exactly the desired
`a - (-5)` reading. No lookahead/whitespace tricks needed.

- `ast.rs`: `UnaryOp::Negate` added.
- `parser.rs`: the `unary` rule's prefix alternative now accepts `just('-')`
  alongside `text::keyword("not")`, both folded generically (the previous code
  discarded the matched operator and hardcoded `Not`; now it's threaded
  through).
- `interpreter.rs`: `UnaryOp::Negate` on a `Number` negates it; on anything
  else, `"Operand of '-' must be a Number, found {type}"` (located, via the
  existing diagnostics renderer).
- `codegen.rs`: mirrors `Not`'s existing runtime type-check-and-trap pattern —
  extract tag, compare to `TAG_NUMBER`, trap if not, else negate via
  `build_float_sub(0.0, num, ...)` (a proven-available builder method in this
  codebase, used instead of the less-certain `build_float_neg`). Also added the
  missing `Negate` arm to the compile-time type-inference match (infers
  `"Number"`).
- **Tests**: `tests/negative_literal.code` (basic literal, precedence,
  subtraction-unchanged, mixed `a - -5`, double negation, negative array
  elements) plus a `-5 * 2 = -10` line added to the existing
  `tests/operator_precedence.code`; `build_negative_literal_exe_runs` in
  `tests/llvm_codegen.rs` for the compiled `--target exe` path. Interpreter and
  codegen parity verified directly, including the type-error path
  (`-true` → located error in both).
- **Full suite green**: `cargo test --workspace` (0 failures), `.code` suite
  138/138 (+1), `llvm_codegen` 19/19 (+1), `code fmt . --check` canonical (156
  files), `code-lsp` still has no `inkwell`/`llvm-sys`.

# T10 — Negative number literals don't parse

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

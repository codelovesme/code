# T7 — README describes semantics the implementation does not have

- **Priority:** High
- **Type:** Documentation / correctness
- **Area:** `README.md`

## Problem

Beyond the operator-name drift (T1/T5), the README documents core language
semantics that the interpreter actively rejects. These are not cosmetic — the
flagship example does not run.

### 1. Variable reassignment (README says yes; language is single-assignment)

README claims `=` reassigns on subsequent use and builds its Memory Model around
it:

- `README.md:120` — "on subsequent use it reassigns"
- `README.md:530,536-540` — Memory Model example `a = 1` then `a = "hello"`
- `README.md:545-549` — the **hello_world** example:
  ```
  a = 1
  a = "hello world"
  assert a ≠ 1
  ```
  shown with output "Program executed successfully."

But variables are **single-assignment**. `tests/fail_reassignment.code` asserts
that `x = 1; x = 2` is an error, and the interpreter reports
`Reassignment is not allowed: 'x' is single-assignment`. The hello_world example
cannot execute.

### 2. String interpolation syntax (`${...}` documented, `$name` implemented)

- `README.md:164-175` documents `"Hello, ${name}!"` with `${}` braces.
- The parser implements bare `$name` (no braces); `tests/string_interpolation.code`
  uses `"hello $name"`. `${name}` fails to parse, and interpolation only accepts
  a bare identifier (not `${obj.field}`).

## Proposed change

- Rewrite the hello_world and Memory Model examples to respect single-assignment
  (use distinct variable names, or demonstrate the error deliberately). Reconcile
  the "reassignment creates a new heap value" prose with the actual model.
- Fix the interpolation section to `$name`, and state that only bare identifiers
  are supported (no `${}`, no field access) — or file a follow-up if `${}` is
  desired as a real feature.

## Acceptance criteria

- Every fenced example in the README runs as written under `code run`.
- No claim of reassignment; interpolation syntax matches the parser.

## Effort

Small–Medium (docs only), but touches the Memory Model narrative.

## Resolution (implemented)

Treated single-assignment as the intended design (the test suite enforces it via
`tests/fail_reassignment.code`). README changes:

- Reworded the `=` description to state single-assignment explicitly.
- Rewrote the Memory Model example and "Immutable Values" principle to drop the
  reassignment narrative.
- Replaced the `hello_world.code` example (was reassignment) with a valid
  single-assignment program, and replaced the stale `=== AST === …` execution
  dump with the actual current `code run` output.
- Interpolation section now documents `$name` (bare identifier only), not `${}`.

Verified the reworked hello_world, interpolation, and range-constraint examples
run under `code run`.

## Open question (still open)

If reassignment is ever *wanted* as a feature, this flips into an implementation
ticket. For now the docs match the enforced behavior.

# Deeply nested values blow the stack, in both output modes

## What happens

```
let big = [1]
big = big + big        -- x14, so 16384 elements
let a = [0]
loop x over big {
    a = [a]            -- each iteration wraps the previous value
}
```

Measured on this machine (8MB stack):

| Nesting depth | `code run` (interpret) | compiled binary |
|---|---|---|
| 4 096 | ok | ok |
| 16 384 | **abort (stack overflow)** | ok |
| 65 536 | abort | ok |
| 131 072 | abort | **SIGSEGV** |

## Why

Every traversal of a value is recursive, one C/Rust stack frame per level of
nesting:

- compiled: `code_release`, `print_json`, `code_values_equal` in
  `src/runtime.c` all recurse into `slot_at(v->items, i)`
- interpreted: `Value`'s *derived* `Drop` recurses through nested
  `Rc<Vec<Value>>` — nothing in `src/value.rs` asks for this, it is just what
  dropping a nested structure does

Nesting depth used to be bounded by source size, because the only way to nest
was to write the brackets. `loop` removed that bound: depth is now whatever
the iteration count is. So this is not a new defect in the traversal code —
it is newly *reachable*, and it arrived with [`loop`](../../src/ast.rs)
rather than with refcounting.

The two thresholds differ because the interpreter's frames are fatter than
the C runtime's, not because the modes disagree about anything.

## Fix direction

Make the traversals iterative with an explicit work stack. `code_release` is
the important one — it is the only one that runs on a path the program can't
avoid. `print_json` and `code_values_equal` matter less (a program that
prints or compares a 100k-deep value is already unusual) but have the same
shape, so it is probably one change, not three.

The interpreter side needs a manual `Drop` impl for `Value` that unrolls the
nesting into a worklist before letting the `Rc`s go — the standard fix for
recursive-drop stack overflow in Rust linked structures.

## Test first

A `.code` fixture along the lines of the snippet above, sized past whichever
threshold is lower. Note that both modes must survive it, and that the
harness runs the interpreter *twice* per fixture, so keep the iteration count
just past the threshold rather than far past it.

# Expression slots pin their last value until the program ends

> **Shipped 2026-08-29.** The original write-up follows, unchanged — its
> measurement is what the result is compared against. "What shipped" at the
> foot records what was built, what it was worth, and the bug it uncovered.

## What happens

Every expression node in the program gets its own `CodeValue` slot in
`main`'s entry block (see `alloc_slot` in `src/codegen.rs`). A slot drops
what it holds when it is *next written*, which for a slot inside a loop body
means every iteration — that is what keeps loops flat. But a slot that is
only ever written once keeps its value until `emit_cleanup` sweeps at exit.

So a chain like:

```
let a = [1]
a = a + a      -- x20
```

keeps all twenty intermediate arrays alive at once, because each `a = a + a`
is a different statement with a different `binop` slot. Peak is roughly
twice the final array rather than one copy of it — measured 161MB for a
2^20-element array that is 84MB on its own.

## Why this was left

It is bounded by *program size* in count, which was the invariant being aimed
for, and it is not a leak: `code_check_leaks` reports zero, and ASan/LSan
find nothing. Only the byte total is larger than it needs to be, and only for
programs that build large values in a chain.

## Fix direction

Release each statement's temporaries at the end of that statement, instead of
at exit.

The blocker is small but real: `code_release` in `src/runtime.c`
**deliberately does not clear `v->heap`** afterwards, because leaving it
alone is what makes `code_copy`'s self-assignment case (`x = x`) work. A slot
released early would therefore be released a second time when it is next
written or swept — a double free. So this needs a separate
`code_clear(slot)` that releases *and* resets the slot to a payload-less
value, used only by the statement-end path.

The other half is telling temporaries apart from permanent slots in codegen:
`gen_let`'s binding slot, and `gen_loop`'s `loopiter`/`loopvar`/`loopidx`
slots, must not be cleared at statement end. A watermark into `Gen::slots`
taken around each `gen_expr` would do it, as long as the slots allocated
outside `gen_expr` are excluded.

Worth checking before starting whether the win justifies it — the chain
pattern above may not be one anyone actually writes.

## What shipped

The fix direction above, with one change of shape: **membership is opt-in
rather than a watermark with exceptions.**

A watermark over every slot allocated during a statement would have had to
subtract the ones that outlive it — a `let` binding, a loop's variable, key,
accumulator and container, a handler's field bindings, a module object — and
getting that subtraction wrong blanks a live variable. So an intermediate now
says so at birth: `alloc_temp` instead of `alloc_slot`. The default answer for
an unclassified slot is the old behaviour, hold it to exit, which is wrong
only in the way this document was already about.

`gen_stmt` takes a watermark into that list, generates the statement, and
clears the range it added. Statements nest, so it is a stack: an inner
statement clears its own range and truncates back, leaving the enclosing one's
entries below it untouched. The clears land wherever the builder ended up,
which is the right place in every case — after a loop's exit block for a loop,
*inside* the body block for a statement within one, and in an unreachable
block after a `break`, where LLVM drops them and the exit sweep still covers
the slots.

The double-free the fix direction predicted is real, and `code_clear` is the
answer it proposed: release, then blank, so that the next write to the slot
and the exit sweep both find a payload-less value. Every slot is still in
`slots` and still swept at exit; the sweep is simply a no-op on a slot the
statement already cleared, which keeps "released exactly once, at exit" true
with no exceptions to state.

### What it was worth

The document's own program, measured before and after on the same machine:

| | peak RSS |
|---|---|
| before | 161.2 MB |
| after | 121.3 MB |

The 161 MB is the sum of every intermediate — twenty arrays, each half the
size of the next, all alive at once. The 121 MB is the floor: the last
`a = a + a` has to hold the 84 MB result and the 42 MB operand at the same
moment, and no slot discipline changes that. So the accumulation is gone
entirely, and what is left is the step itself.

### Was it worth doing

The document asked, and the honest answer to the question as asked is *no* —
the chain pattern is not one anyone writes, and 40 MB on a program that
deliberately builds a 2^20-element array is not a bill anyone was paying.

It was worth doing for a different reason. Releasing a temporary at the end of
its statement is when its heap block gets handed out again, and handing memory
out again is what turns a latent use-after-free into a failing test. It found
one immediately: **`+` on two objects kept the operands' key *pointers*
instead of copying the characters.** That was correct while every field name
was a program literal in read-only data, and stopped being correct on
2026-08-29, when `{ "$name" = v }` began building a name at run time —
`code_object` started copying its keys that day and `code_add` was missed. The
cost was that `acc = acc + { "$k" = v }` in a loop left `acc` naming
characters inside the literal's block, which the next iteration released.
`tests/object_keys.code` had covered exactly that shape since the feature
landed, and passed, because nothing had claimed the freed memory yet.

Both halves are fixed: the merge copies its key characters through the same
`copy_key` the constructor uses, so the two cannot drift again, and the
fixture now allocates objects of the released shape between building the
merged object and reading it, which fails on the old runtime and passes on
this one.

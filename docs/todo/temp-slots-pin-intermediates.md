# Expression slots pin their last value until the program ends

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

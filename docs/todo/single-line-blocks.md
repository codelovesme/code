# A block cannot hold a statement on one line

`if score ≥ 90 { return G { letter = "A" } }` does not parse. The guard
clause — arguably the most-typed shape in any language with early return —
is a syntax error here, and the error does not say why:

```
error: expected end of statement, found RBrace
  --> report.code:12:47
   |
12 |     if score ≥ 90 { return G { letter = "A" } }
   |                                               ^
```

The rule underneath is that `expect_end_of_statement` (`src/parser.rs:571`)
accepts only `Newline` or `Eof`. A statement inside a block must therefore be
followed by a separator before the `}` that closes it. An empty block is
fine, because there is no statement to terminate:

| written | result |
|---|---|
| `if true { }` | parses |
| `if true { let a = 1 }` | **error** |
| `if true { let a = 1; }` | parses |
| `if true {`<br>`    let a = 1`<br>`}` | parses |

## Why this one is worth fixing

**It contradicts the neighbouring rule for the same character.** `{` opens
both a block and an object literal, and the object literal has no such
restriction — `let o = { a = 1 }` is fine inline, and the formatter
deliberately preserves that (`docs/todo/formatter.md`: "a `{ x = 1 }` written
inline stays inline"). Same brace, opposite rules, and the only sentence in
the README a reader is likely to find on the subject is the one about object
literals, which generalises to exactly the wrong conclusion.

**It is undocumented.** The README's "What the language deliberately does not
have" lists no `else`, no `while`, no functions — each an honest, argued
absence. This is not in that list, and reads much more like a consequence of
how statement termination happens to be checked than like a decision anyone
made.

**It costs the shape it blocks.** Four guard clauses:

```
Grade { score } => {
    if score ≥ 90 { return G { letter = "A" } }
    if score ≥ 80 { return G { letter = "B" } }
    if score ≥ 60 { return G { letter = "C" } }
    return G { letter = "F" }
}
```

is 4 lines of body. Written the way the parser requires it is 12, and the
structure is harder to see, not easier — the thing that made the original
readable was that each condition and its answer sat on one line together.

**No `else` sharpens it.** The README's answer to a missing `else` is "write
a second `if`", which makes sequences of single-line guards the idiomatic
conditional in this language. The syntax then refuses to let them be
single-line.

## Options

1. **Accept `}` as a statement terminator inside a block.** `expect_end_of_
   statement` would take `RBrace` alongside `Newline`/`Eof`, without
   consuming it. That is what Rust, Go, C and JavaScript all effectively do,
   and it is a two-line change at one call site. Risk to check: whether any
   currently-invalid program becomes valid in a way that hides a mistake —
   the `1 < 2 < 3` and `a is B is C` cases at `src/parser.rs:585,645` lean on
   this error, so both need a fixture proving they still fail.

2. **Leave the grammar and fix the message.** `expected end of statement,
   found RBrace` becomes something naming the fix: a `;` or a newline before
   `}`. Cheap, honest, and leaves the shape unavailable.

3. **Leave it and document it**, in the README's list of deliberate
   absences, with the reasoning. Only defensible if there is a reason — and
   the write-up above could not find one.

Option 1 is the recommendation. This is the one obstacle in the language that
costs something on the first afternoon and buys nothing back.

## Where it came from

Found by using the language cold — writing a small report program with
handler-based reuse. It was hit twice: once reaching for `loop { ... break }`
on one line, once writing the grade guards above. Both times the error read
as though something was wrong with the expression, not with where the newline
was.

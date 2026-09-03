# The constructs this language does not have get its worst errors

> **Shipped 2026-09-03.** Each is answered by name now. The write-up below
> is the original; "What shipped" records what landed and what it cost.

This language already knows how to answer someone reaching for a construct it
does not have. Two of the best errors in the codebase are exactly that:

```
assert 1 != 2
→ error: unexpected character '!' (inequality is '≠')

assert 1 <= 2
→ error: expected an expression, found '='. The comparison operators are
         '=' '≠' '<' '>' '≤' '≥' — '==', '<=' and '>=' are not operators
```

Both name the replacement. Neither makes the reader guess. `src/lexer.rs:169`
and `src/parser.rs:811`.

That reflex has not been applied to the absences a newcomer actually walks
into first — the ones the README documents under "What the language
deliberately does not have":

| typed | answered with |
|---|---|
| `fn average(xs) { }` | `expected '=' or '+=' after 'fn', found Ident("average")` |
| `def f(x) { }` | `expected '=' or '+=' after 'def', found Ident("f")` |
| `function f(x) { }` | `expected '=' or '+=' after 'function', found Ident("f")` |
| `while i < 3 { }` | `expected '=' or '+=' after 'while', found Ident("i")` |
| `for x in xs { }` | `expected '=' or '+=' after 'for', found Ident("x")` |
| `} else {` | `expected end of statement, found Ident("else")` |

Every one of these is the bare-identifier statement arm at
`src/parser.rs:384`: the parser sees a lowercase word at the start of a
statement, concludes it must be an assignment target, and reports that the
`=` is missing.

## Why the message matters more here than usual

It does not merely fail to help — it points somewhere false. `expected '=' or
'+=' after 'fn'` says `fn` is a fine name and the *assignment* is malformed.
The reader's next move is to try `fn = something`, or to wonder what is wrong
with their function name. Nothing in the message suggests that functions are
not a thing, which is the actual situation and the single largest thing to
learn about this language.

The README is not the gap. It states all of these plainly:

- **No functions.** Handlers, reached by `emit`, are the only call-like thing.
- **No `else`.** Write a second `if`.
- **No `while`.** `loop { }` with `break` is the unbounded loop.

Someone who reads that page start to finish is well served. Someone who does
what people actually do — scaffold a project, start typing — is told they
mistyped an assignment.

## Shape of the fix

A check in the bare-identifier arm, before the `=`/`+=` demand: if the name
is one of a small set of known-absent keywords and what follows does not look
like an assignment, return the message for that keyword instead.

```
fn | def | function | fun | func
    → "there are no functions — a handler answers a particle:
       `Name { field } => { return Result { ... } }`, reached with
       `emit Name { field = x } to this get r`"
while
    → "there is no `while` — `loop { }` with `break` is the unbounded loop"
for
    → "there is no `for` — `loop item over container { }` iterates"
else
    → "there is no `else` — write a second `if`"
```

`else` is the odd one, since it is not at the start of a statement; it lands
in `expect_end_of_statement` (`src/parser.rs:571`) after a block closes. It
needs its own arm there.

The cost is a list of English words in the parser that mean nothing to the
grammar, which is a real if small ugliness — the same one `!`'s error already
accepted. Worth it for the same reason: these are not hypothetical strings,
they are what people type.

## Coverage

`fail_` fixtures per keyword, asserting the program does not run. Those only
prove refusal, not the wording, so the messages themselves want a test in the
style of `tests/error_locations.rs` — one that would fail if the arm were
deleted and the generic message came back.

## Where it came from

Found by using the language cold. `fn` was the first thing typed after
reading the scaffold, on the reasoning that the scaffold's `Greet` handler
looked like a function and there was probably a lighter way to write one.

## What shipped

`absent_construct(name) -> Option<&'static str>` in `src/parser.rs`, consulted
from the bare-identifier arm only when the `=`/`+=` it wanted did not arrive.
Six groups: the function spellings, `while`, `for`/`foreach`, the type
declarations, the import spellings, and the print spellings. `else` is
answered separately from `expect_end_of_statement`, because the `}` it follows
has already closed the `if` body and it never reaches the statement arm.

Two things fell out of doing it that the write-up had not anticipated:

**The caret pointed at the wrong token.** `advance` moves `err_pos` past the
name before the assignment is demanded, so the first version underlined the
word *after* `fn`. The name's position is now captured as it is consumed and
restored when the hint fires. Easy to regress silently, hence its own test.

**None of these words is reserved, and none became reserved.** The hint fires
only where an assignment was expected and did not arrive, so `let print = 1`,
`print = 2` and `print += 1` all still work. That is the difference between
this and adding keywords, and it is the property most likely to be broken by
a well-meaning simplification later — `the_words_are_still_ordinary_identifiers`
exists to catch that.

### Coverage, and why the fixtures were not enough

`tests/fail_absent_{fn,while,for,else,print,import}.code` prove the programs
are refused. That was never in doubt — `fn average(xs)` was refused before
this change too, with the misleading message. So the wording needed a test of
its own: `tests/absent_constructs.rs`, asserting each message names the
construct *and* shows the shape that replaces it.

Verified by mutation. Deleting the `absent_construct` call makes three of the
six tests in that file fail — and every `fail_` fixture still pass, which is
the write-up's point about refusal not being the problem, demonstrated rather
than argued.

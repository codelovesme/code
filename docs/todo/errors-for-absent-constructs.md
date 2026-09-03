# The constructs this language does not have get its worst errors

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

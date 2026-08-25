# code

A small language with six JSON-shaped value kinds, one loop, no functions,
and two output modes that are required to behave identically: `code run`
interprets, `code build` compiles through LLVM to a native binary.

```
let user = { "name": "ada", "wins": [1, 2, 3] }

emit Length { "value": user.wins } to core get n
assert n.value = 3

let name = user.name
let rounds = n.value

link "native_modules/terminal.so" as term
emit Print { "value": "$name won $rounds rounds" } to term
```

> **`old/` is an archive.** The directory `old/` holds a *different*,
> earlier language that happened to share the name — constraints, `∈`,
> handlers written in the language itself. It is kept for reference only.
> Nothing outside `old/` refers to it, and `old/README.md` documents that
> archived language, not this one. If you arrived at a description of
> `code` involving constraints or particles-with-schemas, you were reading
> the wrong file.

## Contents

- [Running it](#running-it)
- [The language](#the-language)
  - [Comments](#comments) · [Values](#values) · [Bindings and scope](#bindings-and-scope)
  - [Strings and interpolation](#strings-and-interpolation) · [Operators](#operators)
  - [Reading members](#reading-members) · [assert](#assert)
  - [if and blocks](#if-and-blocks) · [loop](#loop)
  - [Particles](#particles) · [emit](#emit) · [is](#is)
- [Modules](#modules)
- [What the language deliberately does not have](#what-the-language-deliberately-does-not-have)
- [The two output modes](#the-two-output-modes)
- [Repository layout](#repository-layout)

## Running it

Prebuilt Linux x86_64 tarballs are attached to each [release](https://github.com/codelovesme/code/releases).
To build from source you need Rust and LLVM 17:

```sh
cargo build --release        # target/release/code
cargo test --workspace       # runs every tests/*.code fixture in both modes
```

```sh
code run program.code                      # interpret
code build program.code                    # -> ./program (native executable)
code build program.code --target wasm      # exe | shared | static | wasm
code build program.code -o out/thing
code install terminal                      # fetch a module into ./.code/modules
code ls                                    # what's installed
code remove terminal
code --version
```

The LLVM backend is a Cargo feature (`llvm`, on by default). Without it you
get an interpreter-only build — which is what
[`crates/code-wasm`](crates/code-wasm) is, the engine behind the
[playground](https://codelovesme.github.io/code). `code build` also needs a
system `cc` at runtime, since it hands off final linking.

## The language

### Comments

`--` to end of line. A lone `-` is subtraction or negation.

```
-- this is a comment
let x = 1 - 2
```

### Values

Six kinds, exactly JSON's: **Number** (an f64), **Str**, **Bool**, **Null**,
**Array**, **Object**. There are no type keywords and no declarations of
type; a binding holds whatever it was last assigned.

```
let n = 2.5
let s = "hi"
let b = true
let z = null
let xs = [1, "two", [3]]
let obj = { "x": 1, "nested": { "y": 2 } }
```

Object keys are string literals; values are any expression, so unlike strict
JSON an object literal can reference variables. Objects keep **insertion
order** — iteration and equality both depend on it. Literals may span lines:

```
let nums = [
  1,
  2
]
```

### Bindings and scope

`let` is **mandatory** for a name's first binding. A bare `name = expr` is
reassignment only, and is an error if the name was never declared.

```
let x = 5
x = 6            -- reassigns
y = 1            -- error: undefined variable 'y'
```

That split is what makes shadowing unambiguous. `let` always creates a new
binding in the *current* scope; bare assignment always reaches outward to an
existing one.

```
let x = 1
{
    let x = 2    -- a new, inner binding
    assert x = 2
}
assert x = 1     -- untouched

let y = 1
{
    y = 2        -- reaches out and mutates
}
assert y = 2
```

An undefined variable is caught before the program runs under `code build`,
and at the point of use under `code run`.

### Strings and interpolation

`$name` inside a double-quoted string splices that variable in. Escapes are
`\n`, `\t`, `\"`, `\\`, and `\$` for a literal dollar sign.

```
let who = "ada"
let n = 3
assert "hi $who, $n rounds" = "hi ada, 3 rounds"
assert "costs \$5" = "costs \$5"
```

A `$` that is not followed by an identifier is a lex error, not literal
text — so a stray dollar is reported rather than silently printed. The name
runs to the first character that cannot continue an identifier, and it is a
bare *variable*, never an expression: `"$box.lid"` interpolates `box` and
leaves `.lid` as literal text.

A **Str** splices in bare; every other kind renders as compact JSON, which
means a string *nested* inside an interpolated array or object keeps its
quotes. Interpolation is total — no value is uninterpolable.

```
let s = "hi"
let arr = [1, "a"]
let whole = 3
assert "$s" = "hi"
assert "$arr" = "[1,\"a\"]"
assert "$whole" = "3"          -- numbers: shortest form that round-trips
```

### Operators

| Tier | Operators | Notes |
|---|---|---|
| `or` | `or` | short-circuits |
| `and` | `and` | short-circuits, binds tighter than `or` |
| `not` | `not` | prefix |
| comparison | `=` `≠` `<` `>` `≤` `≥` | **non-associative** |
| `is` | `is` | see [is](#is) |
| additive | `+` `-` | |
| multiplicative | `*` `/` | |
| unary | `-` | negation |
| postfix | `.field` `[index]` | |

Every comparison operator is exactly **one character**: `≠`, `≤` and `≥` are
the real spellings, and `==`, `!=`, `<=`, `>=` are rejected with a message
saying so. The only two-character operator in the language is `+=`.

`=` is both the equality operator and the separator in `let x = …` / `x = …`.
They cannot collide: a statement's `[let] NAME =` prefix is consumed before
expression parsing starts, so every `=` the expression grammar sees is an
equality.

Comparison matches at most one operator, so `1 < 2 < 3` is a parse error
rather than quietly grouping as `(1 < 2) < 3`.

**Operand rules.** Ordering (`< > ≤ ≥`) is Number-only — strings included,
comparing them is an error. Equality (`= ≠`) is the opposite: defined for any
two values, so mismatched kinds are simply unequal. Arithmetic requires
Numbers; `and`/`or`/`not` require Bools. A type mismatch is an error, in
both modes. Division by zero is an error too — the value model is JSON, which
has no way to spell infinity.

`+` is overloaded by operand kind:

```
assert 1 + 2 = 3
assert "a" + "b" = "ab"
assert [1] + [2] = [1, 2]        -- two arrays concatenate
assert [1, 2] + 3 = [1, 2, 3]    -- one array: the other side is an element
assert 0 + [1, 2] = [0, 1, 2]    -- appended or prepended by which side it's on
```

`name += expr` is exactly `name = name + expr`, so it means whatever `+`
means for those values. It is a statement form only, and like a bare
assignment it needs an existing binding.

### Reading members

`.field` and `[index]` read; there is no write-through — `obj.f = v` does not
exist.

```
let point = { "x": 1 }
let nums = [10, 20]
assert point.x = 1
assert nums[0] = 10
assert nums[1 - 1] = 10          -- the index is an expression
```

Two different rules, deliberately:

- **Wrong operand kind is an error.** `.` requires an Object; `[]` requires
  an Array or an Object. `"abc"[0]` and `"abc".length` both fail loudly
  rather than quietly answering null.
- **An absent member is null.** `obj.nope`, `obj["nope"]`, `nums[99]`, a
  non-Number index into an array, a non-Str key into an object — all null.
  The operand kind was right; the lookup just found nothing.

That second half is load-bearing: reading a name a module chose not to export
goes through its alias object as a missing field, and answers null.

An array is keyed by **Number**, an object by **Str** — the same split
`loop` uses.

### assert

`assert <expr>` continues if the expression is `true` and aborts the program
otherwise. A non-Bool is an error, not a falsy value.

```
assert 1 < 2
assert [1, 2] = [1, 2]
assert not false
```

Programs are otherwise silent — there is no print statement in the language
(see [emit](#emit)) — so `assert` is how a fixture states what it means. Every
file in [`tests/`](tests) is a real program that asserts its own expectations.

### if and blocks

```
if x < 10 {
    ...
}
```

There is **no `else`**, and there never will be. The condition must be a
Bool. A bare `{ … }` block is also a statement, and both introduce a scope
that follows the [`let` vs. bare assignment](#bindings-and-scope) rule above.

### loop

One iteration construct, in three shapes.

**Over a container** — an Array or an Object:

```
loop item over [10, 20, 30] {
    ...
}
```

With two names, the first is the **key** and the second the value. The law is
`X[k] = v` for either container, so an array yields a zero-based Number key
and an object yields its field name:

```
loop i, color over ["red", "green"] {   -- i = 0, 1
    ...
}
loop name, score over {"alice": 10} {   -- name = "alice"
    ...
}
```

Names right-align against `(key, value)`, so **one** name always binds the
value, whichever container you are iterating.

**Unbounded** — `loop { }` has no iterable and no bound; only `break` leaves
it. This is how you write what other languages spell `while`:

```
let i = 0
loop {
    i = i + 1
    if i = 5 {
        break
    }
}
```

**Accumulating** — `get name [= init]` declares a binding that starts at
`init` (or null), is assigned freely in the body, and survives the loop.
There is no separate collect form and no `yield`:

```
loop x over [1, 2, 3] get sum = 0 {
    sum = sum + x
}
assert sum = 6

loop x over [1, 2, 3] get doubled = [] {
    doubled += x * 2
}
assert doubled = [2, 4, 6]
```

`break` exits the innermost loop, `continue` starts its next iteration. Both
reach out through any number of enclosing `if`/block bodies — they act on the
enclosing *loop*, not the enclosing block. Outside a loop, either is a parse
error.

### Particles

`ClassName { fields }` — any uppercase-first name immediately followed by
`{` — is **pure parser sugar** for an object literal with a `"_class"` field
prepended. No new value kind, no schema, no validation.

```
let log = Log { "message": "hi" }
assert log._class = "Log"
assert log = { "_class": "Log", "message": "hi" }
```

Because it is only sugar, a particle is structurally equal to a hand-written
object with the same fields in the same order. There is no hidden tag.

### emit

There is no print statement, no file I/O, and no core library of functions.
The way a program reaches the outside world is to `emit` a particle to a
handler:

```
emit Length { "value": [1, 2, 3] } to core get n
assert n._class = "LengthResult"
assert n.value = 3
```

- **`to core`** dispatches to a handler compiled into the runtime itself.
  Core stays deliberately minimal: `Length` (of an Array, or of a Str in
  characters — not bytes) and `Timestamp` (Unix seconds). Every core result
  comes back as a *particle*, never a bare value.
- **`to <alias>`** dispatches to a [linked module](#modules).
- **`get <name>`** binds the result. Without it the result is discarded.

Dispatch is by the particle's runtime `_class`, not by the name written at
the call site — so a particle built elsewhere and passed in a variable
dispatches to the same handler.

A bare uppercase name means the empty particle of that class: `emit Timestamp
to core` is exactly `emit Timestamp {} to core`.

Note `get` is not `as`: `get` names the *result of an emit*, while `as` names
a *linked module*.

### is

`expr is ClassName` is true exactly when `expr` is an object whose `_class`
field holds that name. It is never an error — a wrong class, a missing
`_class`, or a non-object all simply answer false, the same spirit as `=`
being well-defined across mismatched kinds.

```
emit Timestamp to core get t
assert t is TimestampResult
```

The right side is a bare name, not an expression: a class name is a lexical
fact.

## Modules

**Code modules** are `.code` files. Everything in one is private unless it
says `export`:

```
-- shared_values.code
export let greeting = "hello"
export let n = 42
let hidden = 1              -- not visible to anyone linking this
```

```
link "shared_values"              -- flattens exports into this scope
assert greeting = "hello"

link "shared_values" as shared    -- or gather them into an object
assert shared.greeting = "hello"
assert shared.hidden = null       -- private: an ordinary missing field
```

`link` is top-level only. Cycles and duplicate links are errors.

**Native modules** are shared libraries that provide handlers, written in C
against [`src/code_abi.h`](src/code_abi.h) or in Rust against the
[`code-native`](https://crates.io/crates/code-native) crate. They require an
alias, and are reached by `emit`:

```
link "native_modules/terminal.so" as term
emit Print { "value": "hello" } to term get r
assert r.value = 5                -- bytes written

link "native_modules/math.so" as m
emit Sum { "value": [1, 2, 3] } to m get n
assert n.value = 6
```

A native module may also export **variables** (constants), read as ordinary
fields on the alias. They are deep-copied into the host at `link` time, so
`m.answer` is a plain value rather than a live reference into the module:

```
link "native_modules/test_math.so" as m
assert m.answer = 42
assert m.factors = [2, 3, 5]
```

A `.so` works in **both** output modes — the compiled binary `dlopen`s the
very same library the interpreter does. A `.a` static archive is
`code build` only, since there is no `dlopen` for an archive; those fixtures
are named `buildonly_*`.

First-party modules today: `terminal` (print to stdout), `math`, `strings`.
`code install <name>` fetches one into `./.code/modules/`, pinned by sha256
in `./.code/lock.json`; `--global` puts it in `~/.code/modules/` instead. A
`link` reference resolves against a fixed chain — the script's own directory,
then the nearest project's `.code/modules/`, then `$CODE_MODULE_PATH`, then
`~/.code/modules/` — so where a module came from is always answerable.

## What the language deliberately does not have

Each of these is a decision, not an omission waiting to be filled:

- **No functions.** Handlers, reached by `emit`, are the only call-like
  construct — and today they can only be written in C or a native module,
  not in the language itself. (That last part *is* a gap; see
  [`docs/todo/user-defined-handlers.md`](docs/todo/user-defined-handlers.md).)
- **No `else`.** Write a second `if`.
- **No `while`.** `loop { }` with `break` is the unbounded loop.
- **No mutation of a constructed value.** `.field`/`[index]` read only;
  rebuild the value instead.
- **No type keywords, annotations, or declarations of type.**
- **No core I/O.** Reaching the outside world goes through a module, which
  keeps the runtime itself small and the dependency explicit in the source.

Everything currently known to be missing or imperfect is written up, one file
per task, in [`docs/todo/`](docs/todo).

## The two output modes

`code run` interprets; `code build` compiles through LLVM and links a native
binary with the system `cc`. The rule binding them: **every feature must
behave identically in both.**

That is enforced, not aspirational. [`tests/run_language_tests.rs`](tests/run_language_tests.rs)
discovers every `tests/*.code` file and runs it through both paths:

- a plain `foo.code` must succeed in both, and the compiled binary must leak
  nothing — it runs with `CODE_CHECK_LEAKS=1`, so the runtime aborts at exit
  if any heap block survives;
- a `fail_foo.code` must fail in both, whether at compile time or at run
  time;
- a `buildonly_foo.code` is the one sanctioned exception — a `.a`-linked
  module, which must fail under `code run` and succeed under `code build`.

The fixtures are the specification. Each asserts its own expected values, so
"what does this construct do" is answered by an executable file rather than
by prose that can drift.

## Repository layout

```
src/            the language: lexer, parser, ast, interpreter, codegen (LLVM),
                loader (modules), native (dlopen), runtime.c + code_abi.h
tests/          *.code fixtures (the spec) + the harnesses that run them
crates/
  code-wasm/    interpreter-only build for the browser playground (npm)
  code-native/  the crate for writing native modules in Rust (crates.io)
  code-lsp/     diagnostics and semantic tokens over the real lexer/parser
  modules/      first-party modules: terminal, math, strings
site/           the playground; build.py embeds tests/*.code as examples
docs/todo/      open tasks, one file each, written to be picked up cold
old/            archived earlier language — reference only, nothing links to it
```

`src/ast.rs` carries the design decisions and their reasons per construct;
`src/runtime.c`'s header comments cover the compiled value model and its
refcounting rules.

## License

GPL-3.0. See [LICENSE](LICENSE).

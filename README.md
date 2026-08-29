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
> particles with declared schemas. It is kept for reference only.
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
- [Handlers](#handlers)
- [Errors](#errors)
- [Modules](#modules)
- [One canonical layout](#one-canonical-layout)
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
code init                                  # scaffold here; `code init demo` in ./demo
code run program.code                      # interpret
code build program.code                    # -> ./program, beside the source
code build program.code --target wasm      # -t; exe | shared | static | wasm
code build program.code -o out/thing        # --output is the same flag
code build program.code --release          # -r; -O2, the default is unoptimized
code format src/ program.code              # canonical layout, rewritten in place
code format --check tests/                 # writes nothing; non-zero if any differ
code app run demo                          # a directory: runs demo/main.code
code app build demo                        # -> demo/build/demo
code module install terminal               # fetch a module into ./.code/modules
code module ls                             # what's installed
code module remove terminal
code --help                                # or `code help build`, `code build -h`
code --version
```

`run` and `build` take a **file** and answer beside it; `app run` and
`app build` take a **directory**, find its `main.code`, and put artifacts in
`build/`. Two commands rather than one that guesses, because the output
location would otherwise depend on which kind of argument was passed. Both
`app` forms default to the current directory.

`code init` writes three files and nothing else: a `main.code` that **runs
as written** (the obvious template prints, printing needs a module, and a new
project whose first act is a failed `link` is a bad first minute), an empty
`.code/lock.json` — `.code/` is what marks the project root that `link` and
`code module install` resolve against — and a one-line `.gitignore` for the
installed binaries and `build/`, keeping the committed lockfile and dropping
what it can reproduce. An existing file is a
refusal, never a merge.

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

That correspondence is a commitment, not a coincidence: **the set is closed,
and it stays JSON's.** New capability is expressed *with* these six rather
than beside them — a particle is an Object carrying a `_class` field, a
linked module's alias is an Object of its exports, a core handler's answer is
an Object. There is no seventh kind coming, and the two containers stay two:
folding Array and Object into a single ordered map was considered and
rejected (`docs/todo/README.md`), because `+`, serialization and the native
layout all still have to ask which one they are holding.

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

"An error" here means what it means everywhere in this language: the frame
ends and answers with an `Exception`, rather than the program stopping. See
[Errors](#errors).

`+` is overloaded by operand kind:

```
assert 1 + 2 = 3
assert "a" + "b" = "ab"
assert [1] + [2] = [1, 2]        -- two arrays concatenate
assert [1, 2] + 3 = [1, 2, 3]    -- one array: the other side is an element
assert 0 + [1, 2] = [0, 1, 2]    -- appended or prepended by which side it's on
assert {"a": 1} + {"b": 2} = {"a": 1, "b": 2}      -- two objects merge
assert {"a": 1, "b": 2} + {"a": 9} = {"a": 9, "b": 2}   -- right wins, in place
```

The two containers each combine with themselves, and neither borrows the
other's rule. A field both objects name takes the **right** value in the
**left** position — order is part of an object's identity, since equality
compares fields pairwise in order — and merging is one level deep, never
recursive. There is no one-object-operand form to match the array one:
an array can absorb any value as an element, but an object has no key to
file a bare value under, so `{"a": 1} + 3` is an error. With one array and
one object, the array rule wins and the object is simply an element.

Merging is how you copy a particle and change a field, which is the shape
most handler chains want:

```
let edited = received + {"text": "ok"}
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
  rather than quietly answering null — loudly meaning the frame ends with an
  `Exception` (see [Errors](#errors)), not that the program stops.
- **An absent member is null.** `obj.nope`, `obj["nope"]`, `nums[99]`, a
  non-Number index into an array, a non-Str key into an object — all null.
  The operand kind was right; the lookup just found nothing.

That second half is load-bearing: reading a name a module chose not to export
goes through its alias object as a missing field, and answers null.

An array is keyed by **Number**, an object by **Str** — the same split
`loop` uses.

### assert

`assert <expr>` continues if the expression is `true` and fails otherwise. A
non-Bool is an error, not a falsy value.

Failing does not necessarily end the program: inside a handler it ends that
handler, which returns an `Exception` (see [Errors](#errors)). At the top
level, where there is no handler to end, it does end the program.

```
assert 1 < 2
assert [1, 2] = [1, 2]
assert not false
```

Programs are otherwise silent — there is no print statement in the language
(see [emit](#emit)) — so `assert` is how a fixture states what it means. Every
file in [`tests/`](tests) is a real program that asserts its own expectations.

Under `code run`, a failure points at the statement it came from:

```text
error: assertion failed
 --> demo.code:3:1
  |
3 | assert a = b
  | ^
```

The caret finds the top-level statement, so a failure inside an `if` or
`loop` body names the enclosing `loop` rather than the inner line.

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
- **`to this`** dispatches to a handler the program
  [defines itself](#handlers).
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

## Handlers

A handler is the only thing in the language that resembles a function, and
`emit` is the only way to reach one. Core provides two; a native module
provides its own; and a program can define its own with `=>`:

```
Greet { who } => {
    return Greeting { "text": "hi $who" }
}

emit Greet { "who": "ada" } to this get r
assert r is Greeting
assert r.text = "hi ada"
```

**The field list is not optional decoration.** There are no types here to
declare a particle's shape, so without it a body's `who` would be the one
name in the language that appears from nowhere. Listing the fields mirrors
the literal that constructs the particle and gives every name a declaration
site. Anything not listed is simply unreachable from the body.

A listed field the particle doesn't carry is null — the same answer `.field`
gives for an absent member.

The rest of the rules:

- **Top level only**, like `link` — dispatch is one program-wide table, and
  a linked module's handlers join it. A second definition of the same class
  is an error.
- **`return` must yield a particle**, so every result has a class to test
  with `is`. A body that never returns yields null, which is fine: plenty of
  handlers exist for their effect rather than their answer.
- **No matching handler is null**, not an error — the same answer `to core`
  and a native module give. Emitting is not a demand: whether to act on a
  particle is the recipient's business, so a class nothing handles simply
  produces nothing. This reversed on 2026-08-28; see
  [`docs/todo/errors-as-particles.md`](docs/todo/errors-as-particles.md) for
  the model it is the first step of.
- **The body's enclosing scope is the top level**, never the caller's. It
  reads and reassigns top-level bindings and linked module aliases (it must:
  `link` is top-level too, so otherwise a handler could never print), but a
  caller's locals are invisible to it. Ordinary `let` rules apply inside.
- **The handler call graph must be acyclic** — see below.

### No recursion

A handler may emit to another handler, but **no handler may re-enter one that
is already running**: not itself, and not around a longer loop.

```
Third { n } => {
    return Done { "value": n + 1 }
}
Second { n } => {
    emit Third { "n": n } to this get t
    return Done { "value": t.value }
}
First { n } => {
    emit Second { "n": n } to this get s
    return Done { "value": s.value }
}
```

That chain is fine, and so is calling the same handler twice in a row or from
inside a loop — the first call has returned before the next begins. What is
rejected is a cycle:

```
Down { n } => {
    emit Down { "n": n - 1 } to this get inner   -- error, before it runs:
    return Done { "value": 0 }                   -- handler cycle: Down -> Down
}
```

This is what keeps handler calls bounded. With no cycle, the deepest a chain
can reach is the number of distinct handlers in the program, so the stack
cannot run away — where allowing recursion meant a program could overflow it,
which in a compiled binary arrived as a bare segfault with no message.

Cycles are caught **before the program runs**, in both output modes, and
reported as the whole path (`handler cycle: A -> B -> C -> A`) — a refusal,
like any other pre-run error. Because dispatch is by the particle's runtime
`_class`, a particle held in a variable names a handler no static pass can
resolve; those are caught at runtime instead, and a runtime catch is an
answer rather than a refusal: the emit that tried to re-enter gets an
`Exception` back, and the invocation already running is untouched.

## Errors

A runtime error does not end the program. It ends the **frame** — the handler
it happened in — which returns an `Exception` instead of whatever it meant to
return.

```code
Divide { a, b } => {
    return Quotient { "value": a / b }
}

emit Divide { "a": 10, "b": 0 } to this get r
assert r is Exception
assert r.message = "division by zero"
```

`is` is the whole check. There is no `try`, no `catch`, and nothing new to
learn, because an `Exception` is an ordinary particle:

```
Exception { source, message, innerException }
```

`source` names who could not do the work — `"core"` for the language's own
failures, the module's own name for a module's. It is the one field worth
branching on; `message` is prose for a person to read. `innerException`
carries the failure underneath this one, or null.

**Receiving one is not itself an error.** There is no automatic propagation: if
something you emitted to returns an `Exception` and you do not look, you carry
on from where you were.

```code
Outer { } => {
    emit Divide { "a": 1, "b": 0 } to this get r   -- r is an Exception
    emit Print { "value": "still here" } to term   -- and this still runs
    return Report { "inner": r }                   -- pass it on, or don't
}
```

Only the frame where the failure happened unwinds — which makes this a
result-returning model rather than exceptions with unwinding, closer to a
`Result` than to try/catch.

**All three emit targets answer the same way.** A handler you wrote, a linked
module, and `core` each return an `Exception` when they cannot do the work.
None of them can end your program; a module in particular is held to that as a
hard rule (see [Modules](#modules)).

**At the top level there is no frame to return into**, so a failure there ends
the program with a non-zero status — which is what "returned an `Exception`
from the outermost call" amounts to.

```code
assert 1 = 2        -- error: assertion failed, and the program stops
```

### Emitting is not filling in a form

No handler is refused over the fields a particle does not carry. A field that
is not there reads as null — exactly as `.field` does everywhere else — and the
handler runs and answers on that basis.

```code
emit Length { } to core get a
emit Length { "value": null } to core get b
assert a.message = b.message      -- the same particle, so the same answer
```

There is no separate "you did not supply it" complaint, because there is
nothing that could have supplied it: `Length { }` **is**
`Length { "value": null }`, and null has no length.

### What still ends the program before it starts

Errors found before the first statement runs are refusals, not values: a parse
error, an undefined name, a `link` that cannot be resolved, a duplicate
handler, a handler cycle a static pass can see. Both output modes refuse the
same programs, and refusing early is preferred to failing halfway through,
after a program has already had effects.

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

### A module may never end the program

This is the hard rule modules are held to. Whatever goes wrong inside one —
bad input, a failed request, a bug in the module itself — the answer is an
`Exception` handed back to the program (see [Errors](#errors)), never an
exit. A class the module does not handle is null, not a complaint; a field
the particle does not carry is null, so there is nothing for a module to
refuse an emit over.

For a Rust module the rule is *enforced*, not merely asked for: `code-native`
wraps every dispatch in a catch, so even a panic — an `unwrap` on `None`, an
index past the end — comes back as an `Exception` and the program keeps
running. For a C module it is policy only, because a forgotten NULL check
segfaults and an integer `100 / 0` raises SIGFPE, and nothing can catch
either. Rust is therefore the recommended path for anything published; C
remains the ABI's reference implementation.

A module can also **speak first**. If it exports `code_module_set_inbound`,
the host hands it a queue at link time and it may push particles the program
never asked for — which is what an event loop is made of. Those go to the
program's own handlers, not back into the module:

```
link "native_modules/events.so" as ev

Tick { value } => {
    ...
}

emit Start { "value": 3 } to ev get started   -- module queues three Ticks
-- by here they have all been handled
```

Queued particles are dispatched after each top-level statement — and after
each *loop iteration*, which is a statement boundary too — in the order
pushed. **The handler's return value goes back to the module that pushed**,
so a module can ask a question rather than only announce something:

```
link "http_server.so" as srv

Request { method, path } => {
    return Response { "status": 200, "body": "hi from $path" }
}

emit Listen { "port": 8080 } to srv get l
loop {
}
```

Nothing new is written on this side — a pushed particle is answered exactly
as any other, by returning one. A module that wants the answer exports
`code_module_inbound_reply`; most do not, and hear nothing. **A pushed class the program has no handler for is dropped**, not an
error: the module chose to speak, so a message nobody asked to hear is not a
mistake by the program. That is what lets a module report a problem without
every program that links it having to care — `http_client` pushes `Exception` and
`Log`, and [`net_unreachable.code`](tests/net_unreachable.code) handles
neither and passes. Since 2026-08-28 the outbound direction gives the same
answer — `emit` with no matching handler is null — so the two agree rather
than contrast. The cost, accepted deliberately: a module pushing a
*mistyped* class now goes unnoticed.

The queue is bounded at 256 per module, dropping the oldest — a module that
outruns the program costs bounded memory.

A module may push from **a thread of its own**, not only from inside a
dispatch call it was asked on: a timer, a socket accept loop, a terminal
reading keys. A program that wants to receive those has to stay up, and the
way it says so is `loop { }` — the loop that was already in the language,
because a program that wants to stay up writes the thing that stays. Nothing
in the body causes the particles; they arrive because something else is
putting them there:

```
link "modules/timer.so" as timer

Tick { value } => {
    ...
}

emit Start { "value": 3 } to timer get started
loop {
    emit Wait { "timeout_ms": 2000 } to timer get w    -- parks until a push
}
```

**Waiting is the module's job, never the runtime's.** `loop { }` with an
empty body spins a core, exactly as `loop {}` does in Rust — nothing here
sleeps on your behalf or guesses how long you meant to wait. A module that
is an event source blocks inside its own `code_module_dispatch` (a condvar, a
`recv`, an `epoll`) and returns when it has something; the program parks
there at no cost, and the drain at the end of the iteration hands over
whatever was queued meanwhile. `http_client` already blocks this way for an HTTP
round trip.

Two things such a module owes its callers. **Bound the block** — a timeout
field, as `http_client` has: nothing in the ABI can stop a module that blocks
forever, and a module that must be asked before the program may exit is a
module that can hang it. **Expect a backlog** — while one module is parked,
another's pushes queue up behind it, and past 256 the oldest are dropped.

The drain stops at a handler's edge. A loop inside a handler does not drain,
because handing a particle over while a handler is running is re-entry, and
[handlers may not re-enter](#handlers).

### Common particles

A module that pushes cannot know who will receive it, and a program's
handler should be its own definition rather than something shaped by which
modules happen to be linked. So the agreement has to live in the particle,
and two of them are common vocabulary:

```
Log       { source, level, message }    -- level: Info | Warn | Error | Debug
Exception { source, message, innerException }
```

`Exception` is the same particle a failed frame returns (see
[Errors](#errors)) — pushing one and returning one are the same vocabulary,
reached two different ways. `Log` has no returned counterpart: it exists only
to be pushed.

`source` is the module's own name, and it is the module's *data* — not
something the host adds. It exists so one handler can serve every module
without naming any of them:

```code
Log { source, level, message } => {
    emit Print { "value": "[$source] $message" } to term
}
```

That handler works for `http_client` today and for a module written next year, with
no branching and nothing to update when a link is added.

**Extension is additive.** A module may carry extra fields — a handler that
doesn't list them simply never sees them. What breaks the agreement is
*renaming* the common ones.

**If your shape is not the common one, your name should not be either.** A
module with its own kind of record gives it its own class name
(`NetTrace`, not a private `Log`), and a program handles it separately or
not at all — an unhandled push is dropped, so a module's own vocabulary
costs nothing to a program that isn't interested.

This is a convention, not a mechanism. Nothing enforces it, exactly as
nothing enforces `_class` itself. Two modules that both send `Log` with
different shapes will silently mismatch — the second one's fields arrive as
null — which is a bug in the module that ignored the vocabulary, not a
question the language answers. `http_client` is the reference: see
[`crates/modules/http_client`](crates/modules/http_client/README.md).

### Writing one

[`templates/module/`](templates/module) is a working module — a handler, its
fixture, and the CI workflow that publishes it. Copy it, rename `greet`,
replace the handler. `tests/module_template.rs` builds it and runs its
fixture through both output modes on every CI run, so it cannot quietly stop
working against the ABI it is written for.

**A module is GPL-3.0, and that is not a free choice**: every native module
embeds this project's `runtime.c` — that is how the ABI's value-lifetime
contract works — so it is a derivative work. Fine for most people, but worth
knowing before writing one rather than after.

Publishing needs nothing central: tag the repo, CI attaches the artifact and
its `module.json` to a GitHub Release, and a consumer runs `code module install
<url>`. See the [template's README](templates/module/README.md) for the whole
flow and for what to keep when you replace the handler — `guarded`, null for
a class you do not handle, and failures returned as values are the three
rules that make a module unable to break someone else's program.

First-party modules today: `terminal` (print to stdout), `math`, `strings`,
`env` (the environment, so a port or a secret comes from the deployment
rather than the source — see [its README](crates/modules/env/README.md)),
`http_client` (the seven HTTP methods, and `Exception`/`Log` pushed back —
see [its README](crates/modules/http_client/README.md)), and `http_server`
(requests pushed in, answered by what a `Request` handler returns — see
[its README](crates/modules/http_server/README.md)).
`code module install <name>` fetches one into `./.code/modules/`, pinned by sha256
in `./.code/lock.json`; `--global` puts it in `~/.code/modules/` instead. A
`link` reference resolves against a fixed chain — the script's own directory,
then the nearest project's `.code/modules/`, then `$CODE_MODULE_PATH`, then
`~/.code/modules/` — so where a module came from is always answerable.

## What the language deliberately does not have

Each of these is a decision, not an omission waiting to be filled:

- **No functions.** Handlers, reached by `emit`, are the only call-like
  construct — and the only unit of reuse. They take a particle and return
  one; there are no parameters lists, no return-type declarations, and no
  way to hold one as a value.
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

The invariant is written as *behaviour*, but since 2026-08-28 the two modes
agree on their error **text** as well, down to the line, column and caret —
[`tests/message_parity.rs`](tests/message_parity.rs) runs failing programs
through both and compares the whole report. That is not politeness: a failed
frame returns an `Exception` whose `message` the program can read (see
[Errors](#errors)), so two backends wording a failure differently would be a
difference in what a program *computes*.

## One canonical layout

`code format` gives `.code` source a single layout the way `cargo fmt` does
for the Rust half of this repo, and the same CI step enforces it. Editors get
it through [`crates/code-lsp`](crates/code-lsp), which serves the identical
function over `textDocument/formatting`.

It formats the **token stream**, never the AST — which is not an
implementation detail but the reason it is safe to run on your files. The AST
is desugared by design: comments are gone by the time it exists, `n += 1` has
become `n = n + 1`, `Timestamp {}` has become an object literal, and `1.50`
has become an `f64`. A formatter built on it would silently rewrite all four.
Working from tokens, every piece of output is a slice of the input, so
literals keep their spelling and comments survive verbatim.

Hard line breaks stay yours. There is no maximum width and no re-flow: a
`{ "x": 1 }` written inline stays inline, and a multi-line array stays
multi-line. What gets normalized is indentation, spacing between tokens, and
runs of blank lines.

Three properties are checked over every fixture in
[`tests/`](tests), in [`tests/format_fixtures.rs`](tests/format_fixtures.rs):
the token stream is identical before and after (so the meaning cannot have
changed), every comment survives in order, and formatting twice is the same
as formatting once.

A file that does not parse is reported and left alone, never half-rewritten.

## Repository layout

```
src/            the language: lexer, parser, ast, interpreter, codegen (LLVM),
                loader (modules), native (dlopen), runtime.c + code_abi.h
tests/          *.code fixtures (the spec) + the harnesses that run them
crates/
  code-wasm/    interpreter-only build for the browser playground (npm)
  code-native/  the crate for writing native modules in Rust (crates.io)
  code-lsp/     diagnostics, semantic tokens and formatting, over the real
                lexer/parser and the same `code format` the CLI runs
  modules/      first-party modules: terminal, math, strings
site/           the playground; build.py embeds tests/*.code as examples
templates/      module/ — a working starting point for publishing your own
docs/todo/      open tasks, one file each, written to be picked up cold
old/            archived earlier language — reference only, nothing links to it
```

`src/ast.rs` carries the design decisions and their reasons per construct;
`src/runtime.c`'s header comments cover the compiled value model and its
refcounting rules.

## License

GPL-3.0. See [LICENSE](LICENSE).

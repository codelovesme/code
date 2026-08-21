# Linking native modules

`link` today resolves `.code` modules only. The owner wants it to also link
modules compiled by `code` itself or by another language. The format question
was settled while building source linking; the hard part was not.

## Formats — decided

| format | `code run` | `code build` | why |
|---|---|---|---|
| `.so` | yes | yes | **the primary format**: one artifact both modes accept |
| `.a` | no, clear error | yes | static archives cannot be loaded at runtime — there is no `dlopen` for them. Worth supporting on the compile side anyway: it produces a binary with no runtime dependency to find |
| `.wasm` | yes | no, clear error | `cc` cannot link a `.wasm` into a native executable; supporting it would mean embedding a wasm runtime in *every* binary we emit. Natural primary format if a wasm build target is ever added |

The reasoning behind keeping `.so` on both sides, rather than splitting `.so`
for the interpreter and `.a` for the compiler: this language's one standing
invariant is that every feature behaves identically in both output modes, and
the fixture harness enforces it by running everything twice. A split by file
extension would make a program that links `libfoo.so` run under `code run` and
fail to `code build` — the first thing to break that invariant for a reason
that has nothing to do with language semantics. `cc out.o libfoo.so -o exe`
is one extra argument to a link step that already exists.

## The actual blocker: there is nothing to import

A native module can only usefully export **behaviour**, and this language has
no way to invoke any: no functions, no call syntax, and `Expression::Call` was
deliberately removed from the old language with "do not re-add in any form,
sugar or otherwise". Today a linked `.so` could contribute *values* — linking
a shared library to obtain a constant.

The old language solved both halves as one design: `link` brought a module in,
`emit X to <module> get result` invoked it. So **native modules force the
invocation decision**, which was deferred once already (see the "how does this
language express an operation that isn't an operator" thread — `print` and
`len` are the other two faces of it). Do not start native linking without
settling that first; the shape of the descriptor depends on it entirely.

## `CodeValue` becomes a wire format

`src/runtime.c`'s struct is a private implementation detail right now —
`VALUE_SIZE` went 64 → 80 mid-project with nothing to coordinate. The moment a
module compiled elsewhere reads or writes a `CodeValue`, the layout is public
and changing it silently breaks every module built against the old one.

That needs an explicit ABI version — the old language had
`code_module_abi_version()` returning 2, checked at load. Decide it when
native linking starts, not before: versioning a layout nobody depends on is
just ceremony.

## `code build --lib`

`code build` emits an executable with a `main`. Emitting something linkable
needs a library mode, and what it emits follows the table above: `.so` by
default, since that is the one artifact both output modes accept.

## What already accommodates this

Written during source linking specifically so this doesn't need a rewrite:

- `loader::ResolvedModule` is an enum with one variant today. `Native` slots in
  beside `Source` without changing `ModuleResolver`'s signature.
- `Stmt::Import` is documented and implemented as "produce name/value pairs,
  then bind them". Running a body is one way to produce them; reading a
  descriptor is another, and the binding half — flatten vs alias-object —
  works unchanged either way.
- The alias name is kept in the AST rather than erased into an object at load
  time, because `emit ... to <alias>` would need to name the module itself.
- Module paths are quoted strings, so `.so` / `.wasm` / `.dll` need no
  special lexer rule.

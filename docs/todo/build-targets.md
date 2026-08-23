# `code build --target exe|shared|static|wasm`

`code build` produces exactly one thing today — a native executable — with
no flag to ask for anything else. The archived language had
`code build --target ir|exe|shared|static|wasm` (`old/src/main.rs`); the
rewrite kept only `exe`. The owner wants the other three back (`ir` is not
asked for and is not planned here).

## What each target costs, up front

Not a uniform feature — three of the four are small and one is most of the
work:

| target | codegen | link step | real work |
|---|---|---|---|
| `exe` | unchanged | `cc` (today's) | **none** — it is the current behaviour, it just needs a flag |
| `shared` | unchanged | `cc -shared` | small mechanically; see "the semantics question" below |
| `static` | unchanged | `ar rcs` | small mechanically; same question |
| `wasm` | wasm32 triple | `wasm-ld` | **the whole job** — the runtime has no libc to compile against |

The three native targets emit a *byte-identical* object: codegen already
asks for `RelocMode::PIC` (see `compile_to_object`, and the comment there
about PIE), which is what `-shared` needs anyway. So they differ purely in
what runs after codegen, and belong in one enum with one match, not three
code paths.

## The semantics question `shared`/`static` have to answer first

Mechanically wrapping the object in a `.so` is five lines. The problem is
that the object contains **`main`**, and a `.so` whose only entry point is
`main` is close to useless — nothing can call into it except by running the
entire program. The old implementation did exactly this (its `Shared` and
`Static` never reached codegen at all — `BuildTarget` was only ever matched
against `Wasm`; the rest was a different `cc` invocation in `linker.rs`), so
"parity with old" is a low bar that produces an artifact with no consumer.

The useful version is the one this repo's own todo already names as
`code build --lib` ([native-module-linking.md](native-module-linking.md)):
emit a library that satisfies `code_abi.h`, so another `.code` program can
`link "foo.so" as f`. That closes a loop the language already has the
consumer half of.

**But it cannot be complete**, and the reason is worth stating plainly: a
module must export `code_module_dispatch`, and **this language has no
handler-definition syntax**. The old language had `ClassName => { ... }`;
the rewrite has no equivalent — `Stmt` has no handler variant, and `emit` is
strictly a *call*. So a `.code` file cannot implement dispatch. What it
*can* export is values, via `export let`, which is exactly
`code_module_vars`.

So the realistic split is:

- **`export let` → `code_module_vars`** — works, and makes `f.someValue`
  resolve from another program. Genuinely useful.
- **handlers → `code_module_dispatch`** — impossible until the language
  gains a way to define one. Emit a stub that errors with
  `module 'x' defines no handlers`, so the failure is legible rather than a
  missing symbol.

There is one non-obvious obstacle in the useful half: `CodeVarList` is a
**static** structure of `CodeValue`s (`code_abi.h`), but `export let x = <expr>`
is an arbitrary expression evaluated at run time. A compiled library
therefore cannot fill the list statically — it needs a load-time initializer
that runs the program's statements and populates the vars before
`code_module_vars` returns. That is a real design step, not a detail:
today's `main` would become an initializer plus a `main` that calls it.

**Recommendation:** do the mechanical `--target shared|static` (old parity)
in phase 1 so the flag is complete, and treat the module-ABI library as the
separate `--lib` feature it already is in the todo — *not* as something
`--target shared` silently becomes. Two flags, two meanings: `--target`
says what container to produce, `--lib` would say what to put in it.

## `wasm`: the real work

### It does not overlap with `crates/code-wasm`

Worth settling before anyone reads this as duplicated effort. `code-wasm`
compiles the **interpreter** to wasm32 — one large module that can run any
program, given its source, at run time (this is what the playground uses).
`code build --target wasm` compiles **one specific program** to wasm32, with
no interpreter and no parser in the output. Different artifacts, different
sizes, different use cases (embedding a finished `.code` program in a JS
app). Neither replaces the other.

### The blocker: `runtime.c` has no libc on wasm32

`wasm32-unknown-unknown` is freestanding — there is no libc at all. The
first thing that happens is:

```
$ clang --target=wasm32-unknown-unknown -nostdlib -c src/runtime.c
src/runtime.c:15:10: fatal error: 'dlfcn.h' file not found
```

The good news, established by actually auditing `runtime.c` rather than
assuming: **the surface it needs is tiny.** The full list:

| needed | notes |
|---|---|
| `malloc` `calloc` `realloc` `free` | `code_check_leaks` counts live blocks, so `free` has to really free — a bump allocator alone won't do |
| `memcpy` `strlen` `strcmp` | trivial |
| `snprintf` | **only `%s`, `%u`, `%lld`** — verified by grepping every call site |
| `fprintf(stderr, ...)` | one call, in `code_runtime_error` |
| `exit(1)` | same one call site |
| `getenv` | one call (`CODE_CHECK_LEAKS`); can return NULL forever on wasm |
| `time(NULL)` | one call, for the `Timestamp` core handler |
| `dlopen`/`dlsym`/`dlclose`/`dlerror` | **never reached** — see below |

Notably absent: **any floating-point formatting**. `+` in this language
concatenates string+string and adds number+number, and never converts
between them (`code_add`), so the runtime never needs to render a double as
text — the `terminal` module does its own formatting in its own `.c`. That
one fact is what keeps the shim small; a `%g` implementation would have been
most of it.

So: a `wasm_shim.h`, force-included ahead of `runtime.c` with
`clang -include`, supplying the above. Estimate ~200 lines, most of it a
first-fit allocator.

### Native modules are refused, not broken

A program that `link`s a `.so`/`.a` cannot build for wasm — the artifact is
host machine code, and wasm has no `dlopen` regardless. Reject it in
`compile_file_to` with a clear message *before* codegen, rather than letting
it surface as an undefined symbol from the linker. This is also what lets
the `dl*` shims be traps rather than real implementations.

### The import surface is an API decision

Two things the shim cannot implement in freestanding wasm — writing an error
message, and reading the clock — have to become **imports the embedder
supplies**. That makes them a public interface, so name them deliberately
rather than letting `--allow-undefined` invent them:

```
code_host_error(ptr: i32, len: i32) -> ()   // what code_runtime_error writes
code_host_now() -> f64                       // Unix seconds, for Timestamp
```

And the module exports `main() -> i32`. That triple is the whole embedding
contract, and belongs in a short doc section once it works.

### The linker

`wasm-ld --no-entry --export-all --allow-undefined`. It is not installed on
the owner's machine (`apt install lld` provides it), but **`rust-lld` is** —
every rustup toolchain ships it, and `rust-lld -flavor wasm` is the same
LLD. Verified working here:

```
$ rust-lld -flavor wasm --version
LLD 22.1.6
```

So: look for `wasm-ld`/`wasm-ld-NN` first, fall back to `rust-lld` located
via `rustc --print sysroot` (not a hardcoded `~/.rustup` path, which is
wrong for a custom `RUSTUP_HOME` or a distro Rust). A machine that can build
this compiler already has a Rust toolchain, so in practice nothing extra
needs installing — worth doing precisely because it removes the one
"install lld first" step.

## CLI

```
code build <file> [--target exe|shared|static|wasm] [-o <output>]
```

`--target exe` is the default, so every existing invocation is unaffected.
Without `-o`, the extension follows the target (`prog`, `libprog.so`,
`libprog.a`, `prog.wasm`) rather than always using the bare file stem as
`default_output_path` does today.

`--target wasm` needs `clang` and a wasm linker; both absences should be
reported as themselves ("no wasm linker found — install lld, or use a
rustup-managed toolchain") rather than as a failed subprocess.

## Phases

1. `BuildTarget` enum, `--target` flag, `exe`/`shared`/`static`. Small, and
   `exe` is already proven by the whole fixture suite.
2. `wasm_shim.h` + wasm codegen path + linker discovery. The bulk.
3. Fixtures: a `buildonly_`-style prefix cannot express this (that already
   means `.a`-linked), so runner support for "builds for wasm and the module
   validates" needs its own harness hook — probably `wasmtime`/`node` in CI
   rather than a `.code` fixture prefix.
4. Document the embedding contract (imports/exports above).

## Deliberately out of scope

- **`--target ir`** (dump the `.ll`). Old had it; nobody asked for it back,
  and it is one `module.print_to_file` call whenever someone does.
- **`--lib` / module-ABI output.** See the semantics section — it is a real
  feature with a real blocker (no handler syntax), tracked in
  [native-module-linking.md](native-module-linking.md), not smuggled in
  under `--target shared`.
- **wasm32-wasi.** A second wasm target with a real libc, which would make
  the shim unnecessary but adds a sysroot dependency. Only worth it if the
  freestanding shim starts growing.

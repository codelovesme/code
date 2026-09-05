# `code build --target exe|shared|static|wasm`

**Phase 1 shipped 2026-08-24:** the `BuildTarget` enum, the `--target` flag,
and the `exe`/`shared`/`static` link steps exist (`codegen.rs`, `main.rs`,
the `compile` module in `lib.rs`; covered by `tests/build_targets.rs`). Two
refinements over this doc's original sketch: `--target shared` passes
statically `link`ed `.a` modules through to the linker rather than refusing
them (a PIC archive links into a `.so` exactly as into an executable, and a
non-PIC one produces the ordinary relocation error naming its member —
correct behaviour falls out of doing nothing special), and without `-o` the
default output name follows the target (`prog`, `libprog.so`, `libprog.a`,
`prog.wasm`).

**Phase 2 shipped 2026-08-24:** `--target wasm` now emits a real
`wasm32-unknown-unknown` module. The compiler builds the same program object
for wasm, compiles `runtime.c` with the small freestanding shim in
`src/wasm_shim.h`, and links both with `wasm-ld` (or Rust's `rust-lld` when
`wasm-ld` is not installed). The module exports `main` and `memory`, and the
host supplies these imports:

```
env.code_host_error(ptr: i32, len: i32) -> ()
env.code_host_now() -> f64
```

Native `.so`/`.a` links are refused in a wasm build. Modules must instead be
given by the host when the wasm module is created, which is the same approach
used by `crates/code-wasm`.

**Phase 3 shipped 2026-08-30:** `--target shared|static` emits a module-ABI
library rather than a program in a library's clothing. A `.code` file's
handlers become `code_module_dispatch`, its `export let`s become
`code_module_vars`, and another `.code` program links the result exactly as
it links a C or Rust module. This reverses the recommendation below; the
reasoning is under "That recommendation was reversed, 2026-08-30".

`code build` produced exactly one thing before that — a native executable —
with no flag to ask for anything else. The archived language had
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
| `wasm` | wasm32 triple | `wasm-ld` or `rust-lld` | **shipped 2026-08-24** — a small freestanding shim replaces the missing libc |

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

### That recommendation was reversed, 2026-08-30

`--target shared|static` now *is* the module-ABI library, and there is no
`--lib`. What changed is the premise the recommendation rested on: the
paragraph above says "this language has no handler-definition syntax", and
since then it has one — `ClassName { fields } => { ... }`, the thing
`gen_dispatch_body` already turns into a chain for `emit … to this`. The
"impossible half" was the only reason to keep two flags. With handlers
compiling, a library build is not a different *kind* of artifact from what
`--target shared` already produced; it is the same object with the three
`code_abi.h` symbols on the front of it, and a flag whose only job is to say
"and mean it" is a flag nobody would remember to pass.

The obstacle this document did identify is real and was answered the way it
predicted. `CodeVarList` is static and `export let x = <expr>` is not, so
"today's `main` would become an initializer plus a `main` that calls it" —
which is exactly what shipped, with one addition the sketch did not have.
The stream moved into a private `_code_init`, and every entry point that
could be reached *first* — a `.so`'s `main`, `code_module_dispatch`, and
`code_module_vars` — goes through one guarded `_code_lazy_init`. The guard
is not decoration: a consumer reads `code_module_vars` at `link` time,
before it has dispatched anything, so without it the values it copies out
would still be zero.

Two things that fell out of building it, neither obvious from here:

- **A library sweeps nothing.** `emit_cleanup` releases every top-level slot
  as the program's last act, and for a library that act is `_code_init`
  returning — which is *before* `code_module_vars` copies anything out. So an
  exported value was read after its block was freed. Arrays survived it
  silently, a heap string aborted the interpreter in glibc's allocator, and
  a two-value module simply answered wrong. The fix is not a smaller sweep
  but no sweep: `code_abi.h` says a module owns its values for its whole
  lifetime, and a private top-level `let` has to outlive `_code_init` too,
  because a handler may name one.
- **An archive has to hide its own internals.** Both sides of a static link
  generate the same names — `_code_init`, `_code_dispatch_this`,
  `_code_slot_0_num` — so with external linkage a `.a` collides with the
  host it is linked into, and two `.a`s collide with each other. The prefix
  rule in `code_abi.h` makes the *entry points* unique; internal linkage on
  everything else is the other half, and it is also what keeps `loader.rs`'s
  "exactly one symbol ends in `_code_module_dispatch`" true.

**One characteristic, accepted rather than fixed.** A `.a` module shares the
host's single runtime, so the blocks behind its exported values are counted
by the host's `code_check_leaks` and reported when the host exits under
`CODE_CHECK_LEAKS=1`. That is the `.a` contract showing through — the module
is *required* to still own them — and a C module meets the requirement the
same way, with `static` storage;
`tests/native_modules/test_math_static` only escapes it by exporting a single
number. It is why `tests/library_targets.rs` leak-checks the `.so` half and
not the `.a` half. Making it go away needs a "the host is done reading vars"
point in the ABI, which is a bigger change than the problem.

### That point now exists, for the `.so` half — 2026-09-04

`code_abi.h` item 9, `code_module_release`, and a `Shared` build generates
one: it is the sweep this document decided a library must *not* do at
`_code_init`, moved into a function a consumer calls rather than dropped. The
reasoning above is unchanged and is exactly why it had to be a separate
function — `_code_init` returning is not the end of a module's life, and
sweeping there frees an exported value before `code_module_vars` copies it
out. What was missing was any *later* point that is the end of its life.

What made it worth building was `unlink` (`ast::Stmt::Unlink`): a program can
now open an organelle while it runs and close it again, so a module's
lifetime ends before the process's. Without a release point, closing one
would unmap its code and leave its heap behind. `tests/hosted_app.rs` is the
check — a `.code` guest owning heap blocks, started and stopped five times
under `CODE_CHECK_LEAKS=1`, where the guest's own counter is read from inside
its own release point.

The `.a` half is untouched and stays as described above: an archive shares
the host's single runtime, so a second sweep reachable by name would free the
host's slots out from under it.

Covered by `tests/library_targets.rs`, which is a *consumer*: it builds a
module from `.code`, links it from another `.code` program in both output
modes, and lets that program's asserts be the result.

## `wasm`: the real work

### It does not overlap with `crates/code-wasm`

Worth settling before anyone reads this as duplicated effort. `code-wasm`
compiles the **interpreter** to wasm32 — one large module that can run any
program, given its source, at run time (this is what the playground uses).
`code build --target wasm` compiles **one specific program** to wasm32, with
no interpreter and no parser in the output. Different artifacts, different
sizes, different use cases (embedding a finished `.code` program in a JS
app). Neither replaces the other.

### Implementation notes: `runtime.c` has no libc on wasm32

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
text — the `console` module does its own formatting in its own `.c`. That
one fact is what keeps the shim small; a `%g` implementation would have been
most of it.

The shipped `src/wasm_shim.h` is force-included ahead of `runtime.c` with
`clang -include`. It supplies this small surface with a simple linear heap;
the runtime's own leak counter still checks language values, while the
wasm host owns the module's whole memory after execution.

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

And the module exports `main() -> i32` and `memory`. That is the embedding
contract.

### The linker

`wasm-ld --no-entry --export=main --export-memory --allow-undefined`.
The compiler looks for `wasm-ld` first and falls back to **`rust-lld`** —
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

File first, flags after — the same shape as `-o` (shipped 2026-08-24).
`--target exe` is the default, so every existing invocation is unaffected.
Without `-o`, the extension follows the target (`prog`, `libprog.so`,
`libprog.a`, `prog.wasm`) rather than always using the bare file stem as
`default_output_path` does today.

`--target wasm` needs `clang` and a wasm linker; both absences should be
reported as themselves ("no wasm linker found — install lld, or use a
rustup-managed toolchain") rather than as a failed subprocess.

## Phases

1. ~~`BuildTarget` enum, `--target` flag, `exe`/`shared`/`static`.~~ **Shipped
   2026-08-24.** Small, and `exe` was already proven by the whole fixture
  suite; `shared`/`static` get their own checks in `tests/build_targets.rs`
  (a `.so` the dynamic loader accepts and a `.a` holding exactly the program
  object).
2. ~~`wasm_shim.h` + wasm codegen path + linker discovery.~~ **Shipped
  2026-08-24.** `tests/build_targets.rs` builds and runs a wasm module under
  Node, supplying the two host imports documented above.
3. ~~Fixtures: a `buildonly_`-style prefix cannot express this (that already
   means `.a`-linked), so runner support for "builds for wasm and the module

## Deliberately out of scope

- **`--target ir`** (dump the `.ll`). Old had it; nobody asked for it back,
  and it is one `module.print_to_file` call whenever someone does.
- ~~**`--lib` / module-ABI output.**~~ **Shipped 2026-08-30, without the
  flag** — `--target shared|static` is the module-ABI library. The blocker
  named here (no handler syntax) stopped existing; see "That recommendation
  was reversed" above.
- **wasm32-wasi.** A second wasm target with a real libc, which would make
  the shim unnecessary but adds a sysroot dependency. Only worth it if the
  freestanding shim starts growing.

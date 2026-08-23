# Linking native modules

**Phase 1 shipped 2026-08-21: `.so` handlers.** `link "x.so" as x` +
`emit particle to x [get name]` works in both `code run` and `code build`,
via dlopen/dlsym in both (never `cc`-time static linking — see
`runtime.c`'s "Native modules" section and `src/native.rs`). See
`code_abi.h` for the ABI a module implements, and
`tests/native_modules/test_math.c` for a working example. What's below is
what Phase 1 deliberately left out, updated in place rather than rewritten,
since most of the original reasoning still holds.

`link` today resolves `.code` and native `.so` modules. The owner wants it to
also link `.a`/`.wasm` modules and modules compiled by languages other than
`code` itself (a `.so` module is already language-agnostic — any C-ABI
producer works — so that half is actually done; only the format half below
is still open). The format question was settled while building source
linking; the hard part was not.

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

## The actual blocker: there is nothing to import (resolved by `emit`)

A native module can only usefully export **behaviour**, and this language had
no way to invoke any until `emit`/`core` shipped (see memory
`new-code-emit`). `emit particle to <alias> [get name]` is exactly the
invocation this section used to say was missing — Phase 1 reused it
verbatim, `to core` and `to <native alias>` sharing one `EmitTarget` enum in
`ast.rs`.

Phase 1 also **simplified away** the old language's descriptor-table design
(`CodeModuleDesc` enumerating `{class_name, fn_ptr}` handler pairs, read
once at load and dispatched through by the host). Since loading is
dlopen/dlsym in both output modes (see below), and dlsym resolves within one
module's own handle, every module can export the *same* fixed symbol name
(`code_module_dispatch`) and do its own `_class` dispatch internally — no
descriptor, no per-handler symbol naming scheme, no collision between
modules no matter how many are linked. This was confirmed by rereading how
the old language actually avoided handler-name collisions (it also loaded
via dlopen, never `cc`-time static linking) before committing to it here.

## `CodeValue` becomes a wire format (done — `code_abi.h`)

`src/runtime.c`'s struct is no longer a private implementation detail —
`code_abi.h` now holds it (`CodeTag`/`CodeValue`/`CODE_VALUE_SLOT_SIZE`),
`#include`d by `runtime.c` and by every native module. `CODE_ABI_VERSION` is
`1`, checked by `code_native_open` (compiled backend) and `NativeModule::open`
(`src/native.rs`, interpreter). A module signals it by exporting
`code_module_abi_version()`.

A module's result is never adopted by reference — see `code_native_copy_in`
(runtime.c) / `ffi_to_value` (native.rs): a full, host-allocated deep copy,
after which the module's *own* `code_release` (also dlsym'd, so it runs the
module's own allocator bookkeeping, not the host's) frees whatever it
built. This is what keeps `CODE_CHECK_LEAKS` meaningful on both sides of a
dlopen boundary — two copies of this runtime, two separate static
`live_blocks` counters, each only ever freeing blocks it itself allocated.
Also required, and easy to re-forget: every scratch `CodeValue` buffer this
boundary code allocates (`code_native_copy_in`'s `slots`) must be
zero-initialized (`calloc`, not `malloc`) before any constructor that calls
`code_release(out)` first ever touches it — the same hazard documented in
memory `new-code-emit`, hit again and fixed the same way while building this.

A native module's own fatal error (`code_runtime_error`, a crash) takes the
*host* process down with it, `code run` included — there is no sandbox, and
none is planned for Phase 1. Unlike `core`, which the interpreter
reimplements natively in Rust specifically so a bad handler call there is a
clean `Result::Err`, a linked module is real code running in-process; this
is the same tradeoff every native-extension mechanism makes. Documented in
`code_abi.h`; the fixture suite works around it by never provoking a
module's fatal path in-process (no `fail_native_*` fixture exercises a
module's *internal* error — only "wrong alias" and "missing file", both
caught before any module code runs).

## Still open

- **`.a`/`.wasm` formats.** `.so` is the only one `link` accepts today — a
  `.a`/`.wasm` path is a clear compile-time error naming
  `docs/todo/native-module-linking.md`, per the table above. Building these
  needs the reasoning above, adapted: `.a` has no dlopen story at all
  (static-link-only, so the ABI would need to change from
  "dlsym one fixed symbol name" to something that survives normal object
  linking — probably back to unique-per-module symbol names, since two
  `.a`s in the same final binary *do* share one flat symbol namespace,
  unlike two `.so` handles); `.wasm` needs a wasm runtime embedded in the
  interpreter (and, for `code build`, in the emitted binary too) — nothing
  towards either exists yet.
- **Exported variables / types.** Phase 1 is handlers only — a native
  module contributes nothing but an `emit` target, no `x.someConst`. Adding
  it means `Stmt::ImportNative` growing a `vars: Vec<String>` alongside
  `alias`/`path`, populated from a second required export
  (`code_module_vars()` returning name/value pairs) — `Stmt::Import`'s
  "produce pairs, then bind" split was kept general specifically so this
  slots in without changing the binding half.
- **`code build --lib`.** `code build` still only emits a `main`-having
  executable. A program *written in `code`* can't itself become a linkable
  native module yet — moot today anyway, since a `code`-authored module
  would only ever export values (no functions to export as handlers), and
  Phase 1 doesn't support exported variables either (see above).
- **A published `code-native`-equivalent crate/header bundle.** Today
  writing a module means hand-`#include`-ing `runtime.c` (see
  `tests/native_modules/test_math.c`'s doc comment) — fine for a test
  fixture, not yet a documented, versioned, easy-to-depend-on story for a
  real module author.

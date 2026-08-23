# Linking native modules

**Phase 1 shipped 2026-08-21: `.so` handlers.** `link "x.so" as x` +
`emit particle to x [get name]` works in both `code run` and `code build`,
via dlopen/dlsym in both (never `cc`-time static linking — see
`runtime.c`'s "Native modules" section and `src/native.rs`). See
`code_abi.h` for the ABI a module implements, and
`tests/native_modules/test_math.c` for a working example.

**Phase 2 shipped 2026-08-21: exported variables.** A module may now also
export *values* via an optional `code_module_vars()` (see `code_abi.h`'s
`CodeVarList`); the `link` alias binds to an object of those values, so
`x.someConst` works in both modes alongside `emit ... to x`. The alias is
therefore dual-purpose — a field-access target *and* an `emit` target — and
a module that exports no vars still works as an `emit`-only target (the
export is optional, not required). See `tests/native_link_vars*.code` and
`test_math.c`'s `code_module_vars`.

**Phase 3 shipped 2026-08-22: `.a` static modules.** See the "Still open"
section below for the detail — kept there rather than moved up here since
it was written as the answer to what that section used to call the
remaining blocker.

`link` today resolves `.code` and native `.so`/`.a` modules. The owner wants
it to also link modules compiled by languages other than `code` itself (a
`.so`/`.a` module is already language-agnostic — any C-ABI producer works —
so that half is actually done; only `.wasm` (format) and `code build --lib`
(direction) below are still open). Native resolution is no longer
script-directory-only: the fallback chain shipped with the install tooling
(2026-08-23) searches the script directory, the nearest ancestor
`.code/modules/`, `$CODE_MODULE_PATH`, and `~/.code/modules/`, and additionally
maps bare filenames through the project lockfile onto the installed layout
(`<root>/<name>/<version>/<asset>`). Details live in
[community-modules.md](community-modules.md).

## Formats — decided

| format | `code run` | `code build` | why |
|---|---|---|---|
| `.so` | yes | yes | **the primary format**: one artifact both modes accept |
| `.a` | no, clear error | **yes (shipped)** | static archives cannot be loaded at runtime — there is no `dlopen` for them. Worth supporting on the compile side anyway: it produces a binary with no runtime dependency to find |
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
`code_abi.h`. The fixture suite handles it by running the interpret check
as a subprocess (`check_interpret` spawns `code run` and observes the exit
code) — since 2026-08-23's `strings` module, `fail_strings_*` fixtures do
provoke a module's internal error, and the subprocess turns the host-killing
exit into the capturable non-zero status the check wants.

## Still open

- **`.wasm` format (this doc's scope — a *native* `code run`/`code build`
  linking a `.wasm` file).** `.so` and `.a` are the two `link` accepts
  today — a `.wasm` path is a clear compile-time error naming
  `docs/todo/native-module-linking.md`, per the table above. Building it
  would need a wasm runtime embedded in the interpreter (and, for
  `code build`, in the emitted binary too) — nothing towards it exists,
  and 2026-08-22's `crates/code-wasm` work (see below) makes it look less
  worth doing: the actual use case ("a browser app links modules from
  different authors") is already served by `code-wasm`'s own bridge,
  which needs no embedded VM at all. This row stays open only for a
  *native* binary wanting to run wasm plugins, a narrower and so-far
  unasked-for case.

**`crates/code-wasm` gained its own module linking, 2026-08-22 — a
different crate, a different mechanism, not this doc's `.wasm` row.**
`code-wasm` (the interpreter compiled to wasm32, powering the browser
playground and now also a standalone `npm install code-wasm` package —
see `crates/code-wasm/npm/README.md`) can now `link` third-party modules
via `run_with_modules(src, modules)`: each module is a plain, synchronous
JS callback, JSON string in, JSON string out
(`ast::NativeFormat::JsBridge`). Turning an actual `.wasm` file into that
shape is entirely the embedding JS app's job — none of `code_abi.h`'s
pointer/stride ABI applies, and `code`'s own Rust code never touches wasm
bytes. Modules are resolved entirely before the program starts running
(no async inside `link`); `interpreter::Environment::provide_module` /
`link_module` and `interpreter::run_with` are the general hooks this
needed, usable by any future embedder, not just `code-wasm`. Investigating
this also meant properly reading the old language's own `.wasm` story
(`old/src/wasm_module.rs`) for the first time — confirms the byte-offset
ABI approach is real pain worth avoiding, and that it was never even
extended to a browser build there either.

**`.a` shipped 2026-08-22.** `link "x.a" as m` links straight into the
`code build` binary — `code run` refuses it outright (there is no `dlopen`
for a static archive). The real blocker turned out not to be the
module-vs-module symbol collision this doc used to speculate about, but a
module-vs-*host* one: every `code build` binary already links `runtime.c`,
so a `.a` module can't bring its own copy of it (unlike `.so`, which
`dlopen` gives a private symbol table) without duplicate-symbol errors on
every one of runtime.c's ~27 public functions. The fix: a `.a` module calls
the *host's* own constructors directly (now declared `extern` in
`code_abi.h`'s new "`.a` static modules" section) instead of bringing its
own — which also means no deep-copy boundary, no separate allocator, and no
per-module `code_release` the way `.so` needs. The only names a `.a` module
must still choose carefully are its own three entry points
(`<prefix>_code_module_dispatch` etc.), found via `nm` at `code build` time
(`loader.rs`'s `static_module_symbols`) rather than through any new syntax.
See `tests/native_modules/test_math_static.c` for a worked example, and
`tests/buildonly_native_link_static_*.code` / `fail_native_link_static_*.code`
for the fixture coverage (a new `buildonly_` fixture prefix in
`run_language_tests.rs`, for the first feature that must succeed under
`code build` and fail under `code run`).
- **`code build --lib`.** `code build` still only emits a `main`-having
  executable. A program *written in `code`* can't itself become a linkable
  native module yet — moot today anyway, since a `code`-authored module
  would only ever export values (no functions to export as handlers), and
  there's no `code`-side syntax to *declare* a module's exported vars yet
  (Phase 2 reads them from a native module's `code_module_vars`, it doesn't
  let a `code` program define one).
- **A published `code-native`-equivalent crate/header bundle — shipped
  2026-08-22 for Rust.** `crates/code-native` is a Rust crate (`cargo add
  code-native`, published to crates.io) that compiles the vendored
  `code_abi.h`/`runtime.c` via its own `build.rs` and links them into a
  module's `cdylib` — no checkout of this repo needed. It is *not* the
  old language's macro/descriptor-table design (`old/crates/code-native`):
  the new ABI dropped that for one hand-written `code_module_dispatch`
  function, so there's no boilerplate left to generate — this crate is
  safe `CodeValue` builders/readers plus the linking mechanics, nothing
  more. See `crates/code-native/README.md`.

  Distribution is deliberately **per consuming language**, not one
  language-agnostic bundle — mirroring `crates/code-wasm`'s existing npm
  package for JS. C (and anything else that can produce a C-ABI shared
  library) still uses `src/code_abi.h` + `src/runtime.c` directly; C has no
  package registry to target, so that already *is* the C story, unchanged.
  Go, C#, and other languages remain open — nothing has been started for
  them, and each would need its own crate/package-equivalent when someone
  actually needs one (see [no-language-documentation.md](no-language-documentation.md)
  for a similar "written for the case that exists" scoping call).

  `crates/code-native/vendor/{code_abi.h,runtime.c}` are verbatim copies of
  the `src/` files, guarded against drift by
  `tests/native_crate_vendor_sync.rs` at the workspace root (kept out of
  the published crate itself so it can't break `cargo publish`'s isolated
  build check). One implementation wrinkle worth remembering if this ever
  needs revisiting: `cdylib` targets get `--exclude-libs=ALL` from rustc by
  default, which hides symbols pulled in from a *linked static archive*
  (exactly what `build.rs`'s `cc::Build::compile` produces) from the
  dynamic symbol table — even though the crate's own code calls them fine
  internally. Only `code_release` is actually ABI-required to be
  `dlsym`-able (`code_number` etc. never are), so the fix was narrow:
  `build.rs` compiles `runtime.c` with `code_release` renamed via `-D`,
  and `src/lib.rs` re-exports it under the real name from a genuine Rust
  `#[no_mangle]` function, which isn't subject to that exclusion.

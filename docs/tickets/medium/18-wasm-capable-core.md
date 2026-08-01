# 18 — WASM-capable core: feature-gate LLVM and native-`.so` out of the default build

- **Priority:** Medium
- **Type:** Build / portability (enabler; prerequisite for T19 browser playground, also standalone contributor value)
- **Area:** `src/lib.rs`, `src/main.rs`, `Cargo.toml`, `.github/workflows/ci.yml`

## Problem

`code_lang` (the library) cannot be compiled without LLVM today, and cannot be
targeted at `wasm32-unknown-unknown` at all. Two independent blockers:

1. **LLVM import is unconditional in the binary.** `src/main.rs:2` does
   `use code_lang::codegen;` with no `#[cfg(feature = "llvm")]`, and the
   `build` subcommand + its helpers (`build_file`, `build_ir`, `build_native`,
   `build_wasm`, `compile_c_runtime`, `parse_target_flag`, `OutputKind`) call
   into `codegen` directly. So `cargo build -p code --no-default-features`
   fails to compile (verified: `error[E0432]: unresolved import
   code_lang::codegen`). The library's `codegen` module is already gated
   (`src/lib.rs:2`); the binary just doesn't honor it.

2. **Native `.so` loading is not gated at all.** `src/lib.rs` declares
   `native_module` unconditionally, and it uses `libloading`
   (`native_module.rs:312` — `libloading::Library::new`), which requires a real
   filesystem + `dlopen` and does not build for `wasm32-unknown-unknown`. So
   even with LLVM off, the core still won't compile to browser WASM.

(`wasm_module` uses `wasmi`, a pure-Rust interpreter that *does* compile to
wasm32 — nested-WASM execution in a browser build is a separate question,
deferred; see "Out of scope" below.)

## Why this matters independently of the playground

- **Contributor friction:** someone hacking on the interpreter / parser /
  formatter / LSP is forced to install LLVM 17 just to `cargo build`, even
  though none of those touch `codegen`. `--no-default-features` should give
  them a working `run`/`fmt`/`test` build with no LLVM.
- **A real "just run `.code`" persona, not a niche size optimization.**
  Someone who only ever does `code run x.code` (never `code build`) is the
  same shape of user as a Python user running a script — they shouldn't need
  a toolchain that embeds a compiler backend they'll never touch. Measured in
  a throwaway probe crate (`code_lang` linked with `--no-default-features`,
  same `run`/`fmt` code paths as `src/main.rs` minus `codegen`): **4.5M
  stripped on disk, ~1.6M gzip-compressed** — vs. today's full `code` at 42M
  stripped / ~22-25M compressed. Almost all of that difference is LLVM
  (39.7M of the full binary's 42M stripped text is LLVM's statically-linked
  code — confirmed via `ldd`, which shows no dynamic `libLLVM` dependency
  either way; LLVM's cost today is pure size, never a runtime dependency for
  whoever downloads the prebuilt binary).

## Naming decision (2026-08-01)

One binary name, two packages, capability gated by which one you installed —
matching the `dotnet` model (`dotnet` is the same command whether you
installed the Runtime or the SDK; what differs is whether `dotnet build`
works). Concretely:

- **"Runtime" package** — `code_lang` built `--no-default-features` (this
  ticket's build). Binary is named `code`. Has `run`/`fmt`/`test`; `code
  build` prints the "compiled without `llvm`, install the SDK" error from
  the Proposed change below.
- **"SDK" package** — `code_lang` built with the `llvm` feature (today's
  default). Binary is also named `code`. Has everything, including `build`.

No new `[[bin]]` target needed — same source, same binary name, two different
`--features` invocations at release-build time, packaged into two
differently-named tarballs (`code-runtime-*` / `code-sdk-*`). Purely a
packaging-time decision downstream of this ticket's `#[cfg]` gating, not a new
engineering task — the actual `release.yml`/`install.sh` mechanics are tracked
in T17 (which absorbed this split).

## Proposed change

1. `src/main.rs`: gate the `codegen` import and every build-only item behind
   `#[cfg(feature = "llvm")]`. Keep the `"build"` match arm present but split
   its body: under `cfg(not(feature = "llvm"))` print a clear
   "this build was compiled without the `llvm` feature; `build` is
   unavailable" error (do **not** let it fall through to `Unknown command`).
2. `Cargo.toml`: add a `native-so` feature (on by default) gating the
   `libloading`/`native_module` path, so it can be turned off for wasm32.
   Keep `.so` linking in all normal native builds — only the wasm/no-fs build
   turns it off.
3. `.github/workflows/ci.yml`: add a job (mirroring the existing `lsp-no-llvm`
   job) that builds `code_lang` for `wasm32-unknown-unknown` with `llvm` and
   `native-so` off, so this capability can't silently rot the next time
   someone adds an unconditional `codegen`/`libloading` reference — exactly the
   way it's broken today.

## Out of scope

- Actually publishing the "Runtime" package on the release page (the naming
  and packaging shape is decided above, but shipping it is tracked in the
  distribution/website ticket, not this one — this ticket only makes the
  build possible and CI-verified).
- Nested `.wasm` native-module linking (`wasmi`) inside a browser build —
  not needed for playground v1 (single/in-memory source only); revisit if
  remote/native linking is ever wanted in the playground.

## Acceptance criteria

- `cargo build -p code --no-default-features` compiles and produces a `code`
  that runs `run`/`fmt`/`test`, and prints a clear error for `build`.
- `cargo build -p code` (default, with LLVM) is byte-for-byte unchanged in
  behavior.
- `code_lang` (lib, `--no-default-features`, `native-so` off) compiles for
  `wasm32-unknown-unknown`.
- CI enforces both the no-LLVM and the wasm32 builds.

## Effort

Small–Medium — mostly `#[cfg]` gating + one new feature + one CI job. No logic
changes to the LLVM or native paths themselves.

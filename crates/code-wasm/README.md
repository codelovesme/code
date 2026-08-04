# code-wasm

WASM bridge for running a `.code` snippet in a browser or other JS host —
[T19](../../docs/tickets/low/19-browser-playground.md) (browser playground).
Not yet published; not yet wired into any web UI. This is the interpreter
bridge only.

## Scope (v1)

- A single, self-contained snippet — no `link` support (no filesystem in a
  browser; module linking via an in-memory source map is a later slice of
  T19).
- One export: `run_source(src: &str) -> JsValue`, returning
  `{ ok, bindings, diagnostics }`:
  - `bindings`: the program's final top-level variable bindings (this is a
    constraint language with no core I/O — bindings are the only observable
    result of running a program). Each is `{ name, value, kind }`; `value`/
    `kind` are absent if the variable's constraint domain was never narrowed
    to a single value.
  - `diagnostics`: parse or runtime errors, each `{ message, start, end }`
    (char-offset span into `src`, when located).

This JS shape is a public contract once published — see the T19 ticket before
changing it.

## Building — two non-obvious requirements

**Always build `--release`.** A debug build of this crate crashes `rust-lld`
with SIGSEGV while linking, at the toolchain versions this was built against
(rustc 1.96 / LLVM 22 bundled `rust-lld`, `wasm-bindgen` 0.2.126) — reproduced
identically with a separate system `wasm-ld` (LLVM 17), so it isn't a bug in
one specific linker build; it's provoked by this crate's size/export-count in
a debug build. Minimal `wasm-bindgen` repros (with and without
`serde-wasm-bindgen`) link fine in debug — the crash only appears once
`code_lang`'s full parser/interpreter is linked in.

```
cargo build -p code-wasm --target wasm32-unknown-unknown --release
```

**The linked module needs a larger stack than the default.** The parser
(`chumsky` combinator recursion) needs about 16MB of stack — the CLI already
works around this by spawning a thread with a 16MB stack (`src/main.rs`), but
`wasm32-unknown-unknown` has no OS threads to do the same trick, so the stack
has to be sized at *link* time instead. Without this, `run_source` crashes at
runtime with `RuntimeError: memory access out of bounds` (a wasm stack
overflow) on essentially any real program — verified via the smoke test
below. This is handled automatically by `../../.cargo/config.toml`
(`-zstack-size=16777216`, scoped to the `wasm32-unknown-unknown` target only)
— no extra flags needed when building this crate specifically.

## Running the functional smoke test

A build-only check would **not** have caught either issue above — both are
runtime failures, invisible until the module actually executes. `smoke-test/`
runs the real compiled `.wasm` under Node and asserts on `run_source`'s
output (resolved bindings, an unresolved-domain binding, a located parse
error, a located runtime error). CI runs this on every push (see
`code-runtime-and-wasm` in `.github/workflows/ci.yml`); to run it locally:

```bash
cargo build -p code-wasm --target wasm32-unknown-unknown --release

# wasm-bindgen-cli's version MUST exactly match the wasm-bindgen crate
# version resolved in Cargo.lock, or it errors at glue-generation time.
WASM_BINDGEN_VERSION=$(cargo pkgid wasm-bindgen | sed 's/.*@//')
cargo install wasm-bindgen-cli --version "$WASM_BINDGEN_VERSION" --locked

wasm-bindgen --target nodejs --out-dir target/wasm-bindgen-out \
  target/wasm32-unknown-unknown/release/code_wasm.wasm

node crates/code-wasm/smoke-test/run.js target/wasm-bindgen-out/code_wasm.js
```

## Not done yet

- `crates/code-wasm` isn't published as an npm package. Publishing claims a
  real, public package name — that's a deliberate separate step, not bundled
  into this bridge landing.
- No web UI (editor pane + result pane) — see T19's "Proposed change" item 4.
- No module linking (`link` statements) — see "Scope" above; deferred to a
  later T19 slice per the ticket (in-memory `ModuleResolver`, already added
  in `src/module_loader.rs`, not yet wired up here).

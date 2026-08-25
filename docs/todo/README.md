# Open tasks

Things deliberately left undone, with enough context to pick them up cold.
One file per task, named for the problem rather than a number.

Nothing here is a regression: each was either found while building something
else and judged out of scope, or accepted as a known characteristic at the
time. Order below is roughly by how likely it is to bite someone.

| Task | Why it matters |
|---|---|
| [build-targets.md](build-targets.md) | Phases 1 and 2 shipped 2026-08-24: `--target exe\|shared\|static\|wasm` works; wasm uses a small freestanding runtime shim and host-supplied clock/error imports, with Node coverage in `tests/build_targets.rs` |
| [community-modules.md](community-modules.md) | How native modules reach users: core stays minimal (`Length`, `Timestamp`), first-party modules are per-host (`terminal` native / `console` browser, then `math`, `strings`, `net`) on GitHub Releases + npm via `code install`, and the community publish path — direction decided 2026-08-23, phased A–F |
| [emit-bare-particle-name.md](emit-bare-particle-name.md) | Shipped 2026-08-24: `emit Timestamp to core` drops the empty `{}` — a parser-only desugar in the `Stmt::Emit` arm, covered by `tests/emit_bare_particle.code` and two `fail_` fixtures |
| [user-defined-handlers.md](user-defined-handlers.md) | Found 2026-08-25 comparing against `old/`: `Name => { … }`, `return`, `emit … to this/base` all dropped in the rewrite — handlers can only live in C or native modules; spec'd for both backends (old compiled backend had inline label-dispatch too) |
| [inbound-emissions-from-native-modules.md](inbound-emissions-from-native-modules.md) | Found 2026-08-25: dispatch is request/response only; old had per-module `EmitQueue`s + `drain_emissions()` + keep-alive polling so modules could push events (event loops impossible without it) |
| [object-spread.md](object-spread.md) | Found 2026-08-25: `{ ...source, k: v }` gone with the constraint-era object model; the "copy a particle, tweak a field" idiom has no syntax |
| [release-flag.md](release-flag.md) | Found 2026-08-25: old toggled `-O0`↔`-O2` via `--release`; new hardcodes `-O2` (`codegen.rs:595`) — dev builds pay full optimization with no knob |
| [formatter.md](formatter.md) | No canonical layout for `.code` source: `code format [--check]`, token-stream based so comments and `+=` survive, with token-equality + idempotence proven over the fixture corpus and a CI gate beside `cargo fmt` |
| [native-module-linking.md](native-module-linking.md) | Phase 1 (`.so` handlers) + Phase 2 (exported vars) shipped 2026-08-21, Phase 3 (`.a` static modules) + the `code-native` Rust crate shipped 2026-08-22; a *native* `link "x.wasm"`, `code build --lib`, and per-language bundles for anything besides Rust/C still open (a *different* `crates/code-wasm` module-linking story shipped 2026-08-22 too — see that doc) |
| [runtime-error-locations.md](runtime-error-locations.md) | Parse errors point at a line/column since 2026-08-23; runtime ones (`assertion failed`) still don't |
| [temp-slots-pin-intermediates.md](temp-slots-pin-intermediates.md) | Memory held longer than necessary |
| [wasm-fractional-number-text.md](wasm-fractional-number-text.md) | Found 2026-08-25 while shipping string interpolation: interpreter and native `code build` render numbers identically to Rust's `Display`, but `--target wasm` has no libc float formatting, so interpolating a *fractional* number there errors instead |

Done and removed (git log has the detail):

- *deep nesting blows the stack* — every traversal of a value in both
  runtimes is now iterative, covered by `tests/stress_deep_nesting.code`.
- *stress fixtures become playground examples* — `stress_*` joins `fail_*`
  as a prefix `site/build.py` holds back, and the two generated fixtures
  were renamed into it.
- *string interpolation* — `"hi $name"` splices variables again, with `\$`
  for a literal dollar and a bare `$` a lex error rather than silent literal
  text. Rendering agrees byte-for-byte between the interpreter and native
  `code build`; the wasm gap it exposed is its own entry above.
- *no language documentation* — the root `README.md` now documents the
  language as it actually stands, states plainly that `old/` is an archive,
  and covers the deliberate omissions so they don't read as gaps. Every
  example in it was executed in both output modes, and every error it
  promises was checked to actually error.

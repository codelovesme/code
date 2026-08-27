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
| [inbound-emissions-from-native-modules.md](inbound-emissions-from-native-modules.md) | Phases A and B shipped 2026-08-25: a module exports `code_module_set_inbound` and pushes particles, both modes drain between top-level statements and dispatch to the *program's* handlers, bounded at 256 dropping oldest. Still open: the keep-alive loop (daemons, and pushing from a module's own thread), plus the old `emit … to base` — an *upward* edge in the module graph |
| [formatter.md](formatter.md) | No canonical layout for `.code` source: `code format [--check]`, token-stream based so comments and `+=` survive, with token-equality + idempotence proven over the fixture corpus and a CI gate beside `cargo fmt` |
| [native-module-linking.md](native-module-linking.md) | Phase 1 (`.so` handlers) + Phase 2 (exported vars) shipped 2026-08-21, Phase 3 (`.a` static modules) + the `code-native` Rust crate shipped 2026-08-22; a *native* `link "x.wasm"`, `code build --lib`, and per-language bundles for anything besides Rust/C still open (a *different* `crates/code-wasm` module-linking story shipped 2026-08-22 too — see that doc) |
| [runtime-error-locations.md](runtime-error-locations.md) | Option 1 shipped 2026-08-27: under `code run`, a runtime error points at the top-level statement it came from (a nested failure reports the enclosing one). Still open: `code build`, whose errors come from `runtime.c` and stay bare — so the two modes now differ in how well they *report* an error, though not in which programs error |
| [temp-slots-pin-intermediates.md](temp-slots-pin-intermediates.md) | Memory held longer than necessary |
| [wasm-fractional-number-text.md](wasm-fractional-number-text.md) | Found 2026-08-25 while shipping string interpolation: interpreter and native `code build` render numbers identically to Rust's `Display`, but `--target wasm` has no libc float formatting, so interpolating a *fractional* number there errors instead |

Done and removed (git log has the detail):

- *object spread* — **decided against, 2026-08-26**, the same day `obj + obj`
  started merging. Spread existed to spell "copy this object, change a few
  fields"; `source + {"k": v}` now spells it, and `base + Reply {}` re-tags a
  particle since `_class` is an ordinary field. What was left was a second
  syntax for something the operator already does, so it goes. Nested spread
  (`{ ...a, ...b }`) has no `+` equivalent, but that was already out of scope
  when the ticket was written. The old language's implementation is still in
  `old/` if this is ever reopened.

- *merge `Array` and `Object` into one container* — **decided against,
  2026-08-26.** The two are already one container conceptually (`[]` and
  `loop`'s `X[k] = v` law accept both; `CodeValue` is a single struct whose
  `keys` is NULL for an array), but merging them buys no simplicity: the
  array-vs-object question is still asked by `+` — which as of 2026-08-26
  carries *two* container rules, concatenation for arrays and
  merge-with-override for objects, and a single type would have to pick one
  — by any future serializer (`[]` or `{}` for an empty container — PHP's
  permanent bug), and by the native layout. A
  merge relocates that question from one type tag into three runtime shape
  sniffs. Standing principle instead: **the value model stays exactly
  JSON's six kinds**, and anything new is expressed with them rather than
  beside them. Costs kept: the paired traversals in `value.rs` and
  `runtime.c`, and `LoopIter`'s two variants.

- *`code build --release`* — **shipped 2026-08-27.** The opt level was
  hardcoded at `-O2`; it is now `-O0` by default with `--release` for `-O2`,
  matching `old/`. The tradeoff taken knowingly: build-and-ship in one step
  now produces an unoptimized artifact unless the flag is remembered, the
  same bargain `cargo` makes. Detail in
  [release-flag.md](release-flag.md), which stays as the record.

- *deep nesting blows the stack* — every traversal of a value in both
  runtimes is now iterative, covered by `tests/stress_deep_nesting.code`.
- *stress fixtures become playground examples* — `stress_*` joins `fail_*`
  as a prefix `site/build.py` holds back, and the two generated fixtures
  were renamed into it.
- *string interpolation* — `"hi $name"` splices variables again, with `\$`
  for a literal dollar and a bare `$` a lex error rather than silent literal
  text. Rendering agrees byte-for-byte between the interpreter and native
  `code build`; the wasm gap it exposed is its own entry above.
- *user-defined handlers* — `ClassName { fields } => { body }` and
  `emit … to this`, in both backends. Two claims in the old write-up turned
  out to be wrong and are corrected in the git history: duplicate handlers
  were always an error in `old/` (not "stacked, last non-null wins"), and
  `to base` meant the linking *module*, not an enclosing handler — that half
  moved to the inbound-emissions entry above. `Exception`/catchable asserts
  stay unported: making a failed assert catchable is a try/catch design
  decision of its own, not handler mechanics.
- *no language documentation* — the root `README.md` now documents the
  language as it actually stands, states plainly that `old/` is an archive,
  and covers the deliberate omissions so they don't read as gaps. Every
  example in it was executed in both output modes, and every error it
  promises was checked to actually error.

# Open tasks

Things deliberately left undone, with enough context to pick them up cold.
One file per task, named for the problem rather than a number.

Nothing here is a regression: each was either found while building something
else and judged out of scope, or accepted as a known characteristic at the
time. Order below is roughly by how likely it is to bite someone.

| Task | Why it matters |
|---|---|
| [single-line-blocks.md](single-line-blocks.md) | **Shipped 2026-09-03.** A `}` now ends the statement it closes, so `if score ≥ 90 { return G { letter = "A" } }` parses — the guard clause, which with no `else` is how a multi-way conditional is written here. A pure relaxation guarded on `block_depth`, so a stray top-level `}` stays the error it was. **`;` went with it**: nothing required it once a `}` could end a statement, and nothing in 274 `.code` files had ever used it as a separator, so the second spelling is gone and typing one now names the first. Found by using the language cold |
| [errors-for-absent-constructs.md](errors-for-absent-constructs.md) | `fn`, `def`, `function`, `while`, `for` all answer `expected '=' or '+=' after 'fn', found Ident(...)` — the bare-identifier arm at `src/parser.rs:384` reads them as assignment targets, so the message says the *assignment* is malformed rather than that functions do not exist, which is the largest single thing to learn here. The reflex already exists and is excellent for `!` → `≠` and `<=` → the operator list; it has simply never been pointed at the absences the README documents |
| [silent-typos-at-the-particle-boundary.md](silent-typos-at-the-particle-boundary.md) | `assert nope = 1` errors, but `emit Grdae { score = 88 } to this get g` binds null and says nothing, and `emit Grade { scoer = 88 }` runs the handler with `score` null. Both behaviours are load-bearing — an unhandled class must be droppable, a missing field must read as null — so the gap is that nothing distinguishes a deliberate no-answer from a typo at what is, with no functions, this language's call boundary. `to this` is statically checkable and `verify.rs` already walks the program |
| [build-targets.md](build-targets.md) | Phases 1 and 2 shipped 2026-08-24: `--target exe\|shared\|static\|wasm` works; wasm uses a small freestanding runtime shim and host-supplied clock/error imports, with Node coverage in `tests/build_targets.rs`. **Phase 3 shipped 2026-08-30**: `shared`/`static` now emit *module-ABI libraries* — a `.code` file's handlers become `code_module_dispatch` and its `export let`s `code_module_vars`, so another `.code` program links it exactly as it links a C or Rust module. This reverses the document's own recommendation, which had argued for a separate `--lib`; the premise it rested on ("this language has no handler-definition syntax") stopped being true. The stream moved into a private `_code_init` behind one guarded `_code_lazy_init`, because a consumer reads `code_module_vars` at `link` time, before it has dispatched anything. Two bugs found on the way: **a library must sweep nothing** — `emit_cleanup` freed every top-level slot when `_code_init` returned, which is *before* `code_module_vars` copies anything out, so an exported value was read after its block was freed (arrays survived it silently, a heap string aborted the interpreter in glibc's allocator, a two-value module answered wrong) — and **an archive must hide its own internals**, since both sides of a static link generate `_code_init`, `_code_dispatch_this` and `_code_slot_0_num` alike. One characteristic kept: a `.a`'s exported heap values are counted by a leak-checked host, which is the `.a` contract showing through. Covered by `tests/library_targets.rs`, which links a `code`-built module from `code` |
| [community-modules.md](community-modules.md) | How native modules reach users: core stays minimal (`Length`, `Timestamp`), first-party modules are per-host (`terminal` native / `console` browser, then `math`, `strings`, `http_client`) on GitHub Releases + npm via `code install`, and the community publish path — direction decided 2026-08-23, phased A–F |
| [emit-bare-particle-name.md](emit-bare-particle-name.md) | Shipped 2026-08-24: `emit Timestamp to core` drops the empty `{}` — a parser-only desugar in the `Stmt::Emit` arm, covered by `tests/emit_bare_particle.code` and two `fail_` fixtures |
| [inbound-emissions-from-native-modules.md](inbound-emissions-from-native-modules.md) | Phases A and B shipped 2026-08-25: a module exports `code_module_set_inbound` and pushes particles, both modes drain between top-level statements and dispatch to the *program's* handlers, bounded at 256 dropping oldest. `.a` static modules joined on 2026-08-28 (`code_static_open` gives one the queue its missing `dlopen` handle used to hold). **The keep-alive loop closed 2026-08-29**, in two halves the same day: a loop iteration became a statement boundary, and then a module gained the right to push from a thread of its own (a locked ring, an atomic leak counter, a *thread-local* release stack — the one the design didn't predict — and a 1ms wait for an empty `loop { }` that was built and then deleted the same day — the runtime does not guess how long a program meant to wait, Rust's `loop {}` burns a core too, and a module blocking inside its own dispatch already delivers the same 0 CPU with no host code at all). A module that can speak first is now never unloaded, because `dlclose` would pull the ground from under its thread. **`emit … to base` closed the last open piece 2026-08-29**: a linked module sends a particle up exactly one level of the module graph, answered by the direct parent's own handlers, refused anywhere else by the shared `verify.rs` check. Still open: two modules pushing one class with different shapes, accepted as a convention rather than fixed |
| [errors-as-particles.md](errors-as-particles.md) | **All five phases shipped 2026-08-28.** A runtime error ends the *frame*, which returns `Exception { source, message, innerException }`; the program only ends when there is no frame left. Result-returning, not unwinding — `∈ Exception` is the whole check, and a returned Exception does not propagate on its own. All three emit targets answer the same way, and no module can end the program. What remains open is listed at the foot of that file: `code_field`/`code_index` in the module ABI, a non-particle `emit` still splitting three ways, and the deliberately-fatal cases (out of memory, module load failure, the wasm number-text gap) |
| [formatter.md](formatter.md) | **Shipped 2026-08-28.** `code format [--check] <paths>`, formatting the token stream so comments, `+=` and particle sugar survive — the AST is desugared and would eat all three. Token equality, comment preservation and idempotence proven over the corpus *before* it was reformatted, plus two tests that stop those passing vacuously; the CI gate sits beside `cargo fmt --all --check` |
| [native-module-linking.md](native-module-linking.md) | Phase 1 (`.so` handlers) + Phase 2 (exported vars) shipped 2026-08-21, Phase 3 (`.a` static modules) + the `code-native` Rust crate shipped 2026-08-22; a *native* `link "x.wasm"`, `code build --lib`, and per-language bundles for anything besides Rust/C still open (a *different* `crates/code-wasm` module-linking story shipped 2026-08-22 too — see that doc) |
| [runtime-error-locations.md](runtime-error-locations.md) | **Closed 2026-08-28.** Both modes point at the top-level statement a runtime error came from, byte-identically (a nested failure reports the enclosing one, in both). The compiled half turned out small because phases 3–4 of errors-as-particles left `code_abort_failure` as the only place a compiled program reports anything, so the location needed one global rather than threading through every call site. `tests/message_parity.rs` compares the two backends' whole stderr |
| [temp-slots-pin-intermediates.md](temp-slots-pin-intermediates.md) | **Closed 2026-08-29.** A statement now releases its own intermediates when it ends, instead of leaving them for the exit sweep: `alloc_temp` marks a slot as one nobody can name afterwards, `gen_stmt` clears the range it added, and `code_clear` blanks as well as releases so the next write and the exit sweep both find a payload-less value. Membership is opt-in rather than the watermark-with-exceptions the write-up proposed — the exceptions were bindings, and getting one wrong blanks a live variable. The doc's own program went from 161 MB to 121 MB, which is the floor for it (`a = a + a` must hold both arrays at once); what is gone is the *accumulation* of every earlier intermediate. The win itself did not justify the work — nobody writes that chain — but handing memory back is what exposes a use-after-free, and it found one straight away: `+` on two objects kept the operands' key *pointers* rather than copying the characters, which `code_object` had started doing the same day computed keys landed. `tests/object_keys.code` had covered that shape all along and passed by reading freed memory nothing had claimed yet |
| [wasm-fractional-number-text.md](wasm-fractional-number-text.md) | **Closed 2026-08-29**, by neither route the document proposed — both would have built a *second* implementation of number-to-text and then had to argue it agreed with the first. The algorithm stayed exactly where it was; the two things it cannot compute freestanding — the exact expansion of a double, and reading one back — became host imports beside the clock and the error sink (`toExponential(40)` and `Number()` in JavaScript, one call each). The rounding rule that has to match Rust's `Display` never left C, so the modes agree by construction. Verified over 205k random doubles, native against wasm, byte-identical; `tests/build_targets.rs` now runs `interp_number_text.code` under Node. Cost: the wasm target requires four host functions rather than two, and there is no making one optional — a weak undefined symbol is dropped rather than imported |

Done and removed (git log has the detail):

- *object spread* — **decided against, 2026-08-26**, the same day `obj + obj`
  started merging. Spread existed to spell "copy this object, change a few
  fields"; `source + {k = v}` now spells it, and `base + Reply {}` re-tags a
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
  moved to the inbound-emissions entry above. ~~`Exception`/catchable asserts
  stay unported.~~ Reversed 2026-08-28 — see the errors-as-particles entry
  above. The reasoning here was that catchable asserts meant a try/catch
  design; what shipped needs no `try` and no `catch`, because a failed frame
  simply *returns* an `Exception` and `∈` already tests it.
- *no language documentation* — the root `README.md` now documents the
  language as it actually stands, states plainly that `old/` is an archive,
  and covers the deliberate omissions so they don't read as gaps. Every
  example in it was executed in both output modes, and every error it
  promises was checked to actually error.

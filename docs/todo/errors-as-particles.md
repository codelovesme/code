# Errors are values: a frame ends, the program does not

Decided 2026-08-28. This reverses a documented decision — the root README
lists catchable asserts among what the language deliberately lacks, and
`inbound-emissions-from-native-modules.md` records `Exception` as
explicitly unported. That text goes when this lands.

## The model, in one rule

> A runtime error does not end the program. It ends the **frame**, which
> returns `Exception { message, innerException }`.

- Raised inside a handler → that handler returns the `Exception`. Its caller
  receives it through `get`, like any other result.
- Raised at the top level of the entry program → there is no frame left to
  return to, so the program ends, **exit non-zero**.

Two consequences that make this small rather than sweeping:

**Receiving an `Exception` is not itself an error.** It is an ordinary
value. The caller may test it, ignore it, or pass it on. There is **no
automatic propagation**: if C returns an `Exception` to B and B does not
look, B carries on from where it was. This is a result-returning model, not
exceptions with unwinding — closer to Go or Rust's `Result` than to
try/catch.

**Nothing new to write.** `is` already does the check the model needs:

```code
emit Greet { "who": "ada" } to this get r
if r is Exception {
    emit Print { "value": r.message } to term
}
```

And "a handler's `return` must yield a particle" still holds unchanged —
`Exception` is a particle.

## What each decision settles

| | Rule |
|---|---|
| Expression errors (`1 + "a"`, `x / 0`, `.field` on a number) | same as any error: the binding never happens, the frame returns `Exception`. Values are never poisoned |
| Top-level | program ends, non-zero — "finished with an error" |
| A callee's `Exception` | a plain value; caller continues unless it chooses otherwise |
| `emit` with no `get` | the `Exception` is discarded, silently, by the caller's choice |
| No handler anywhere (`to this`, `to core`, `to <alias>`) | **null, not an error.** Sending a particle is not a demand: whether to act on it is the recipient's business |
| Module with a handler but unusable input | `Exception`. "I don't do `Ping`" and "I do `Get` but you gave me no `url`" are different answers |
| `assert` that passes | nothing changes; execution continues to the next statement |
| Pre-run errors (parse, verify, link, cycles, duplicate handler) | still errors, unchanged |

## `Exception`'s shape

```
Exception { message, innerException }
```

No `source`. A returned value does not need to say who it came from — the
caller wrote `to net` and knows. `innerException` lets a failure carry the
one beneath it.

This differs from the `Exception { source, message }` in the root README's
"Common particles" section, which describes what `net` pushes *today*. Both
change together when this ships. `Log` keeps its `source`: a *pushed*
particle has no caller to infer it from, which is exactly the asymmetry.

Line and column belong inside `message` — `span::render` already produces
that text for runtime errors, so it comes for free.

## Fixture impact

**In-handler asserts: none.** An earlier count of 13 in this document was
wrong — it matched any `assert` appearing after a `=> {` anywhere in the
file. Scanning handler *bodies* properly finds exactly one
(`handler_basic.code`), and its assert passes, so it behaves identically
either way.

**Unknown-handler fixtures: four, and they invert.** These pass today
*because* an unmatched class is an error; under the new rule they get null
and succeed, so they stop testing anything:

```
fail_emit_bare_unknown_handler.code   unknown core handler 'Foo'
fail_emit_unknown_handler.code        unknown core handler 'Foo'
fail_handler_unknown.code             no handler defined for 'Nobody'
fail_net_unknown_handler.code         net: unknown handler 'Delete'
```

They must be re-stated as `emit … get r` + `assert r = null` — asserting
the new rule rather than the old one — *before* the semantics move.

**The other 72 `fail_*` fixtures** fail through top-level errors and keep
working: a top-level error still ends the program non-zero. That is what the
non-zero exit requirement protects, since the harness reads nothing but exit
status.

## Implementation cost, honestly

**The interpreter is the easy half.** Errors are already `Result<_, String>`
threaded through every call, and `Flow` already has a `Return(Value)`
variant for `return`. Turning an error into `Flow::Return(Exception)` at the
frame boundary is a contained change.

**The compiled backend already has the unwinding machinery.** ~~It has none
at all.~~ Corrected 2026-08-28 while starting phase 3, by reading
`gen_return` instead of assuming: it copies the value into `frame.out` and
branches to `frame.exit`, which is exactly the shape a failing frame needs.
Cleanup is simpler than feared too — slots are function-lifetime, released in
one `emit_cleanup` sweep, so every frame has exactly one landing point and it
is already built.

The real gap was narrower: a failure discovered *inside* a C helper had no
way to say so. `code_runtime_error` is `_Noreturn`, so `code_div` could not
tell its caller anything — there was no caller left. Giving it a way to speak
is what phase 3 is, and it is the bulk of the work.

Until that exists, the two output modes would disagree about which programs
end — which the fixture harness *cannot catch*, since it only compares
pass/fail and both modes would still "fail", just differently. Any phase
that changes error semantics needs its own both-mode fixture.

## Phasing

1. ~~**No handler yields null**~~ — **shipped 2026-08-28.** `to this`, `to
   core` and `to <alias>` all answer null. Interpreter: `dispatch_handler`
   and the core arm return `Value::Null`. Codegen: `_code_dispatch_this`
   writes null on fallthrough, and a program with *no* handlers at all
   emits a null instead of refusing to compile. `runtime.c`'s core dispatch
   likewise. Every module — `net`, `strings`, `math`, `terminal`, and the
   four test doubles — returns null for a class it does not handle instead
   of calling `code_runtime_error`.

   The drop flag added to `_code_dispatch_this` earlier the same day was
   removed again: both callers now want the same answer, so the parameter
   had nothing left to distinguish.

   The four inverted fixtures became three that assert the new rule:
   `emit_unknown_handler_is_null.code` (all three targets, plus the bare
   spelling and the no-`get` case), `emit_unknown_handler_is_null_no_handlers.code`
   (a program with no handler at all — the compiled backend has no dispatch
   chain to fall through there, so the null has to come from elsewhere), and
   `net_unknown_handler_is_null.code`.
2. ~~**Modules return `Exception` instead of aborting**~~ — **shipped
   2026-08-28.** All 44 module-side `code_runtime_error` calls are gone:
   `net` 12, `strings` 9, `math` 6, `terminal` 3, and the test doubles 14.

   New in the ABI: `code_make_exception(out, source, message, inner)` in
   `runtime.c`, `exception`/`exception_wrapping` in `code-native`, and
   `code_str_owned` **promoted from `static` to the module-facing header**.
   That last one was found by building an `fs` prototype and hitting it:
   `code_str` only *borrows* its pointer, so an exception message built into
   a stack buffer dangles the moment the handler returns, and the symptom is
   a silently truncated string rather than a crash. Every module now builds
   a dynamic message, so every C module author would have hit it.

   **`guarded` closes the panic hole.** A panic escaping an `extern "C"`
   function aborts rather than unwinding — measured: the host's
   `catch_unwind` never runs, the process dies with *"thread caused
   non-unwinding panic. aborting"*. So the catch cannot live in the host, and
   `code-native::guarded` puts it on the module's own side of the boundary.
   `tests/native_modules/test_panics` + `tests/panics_become_exceptions.code`
   keep it honest: `unwrap` on `None`, an index past the end, and a runtime
   division by zero all return `Exception` and leave the program running.

   **`net` lost its validation pass entirely**, per the "there is no such
   thing as misuse" decision: a field the particle does not carry is null,
   and a null url is a url that cannot be fetched, so the failure comes from
   attempting the request. Non-String values are rendered rather than
   refused. The messages improved — `bad uri: 42 is missing scheme` against
   the old `requires a string 'url'`. `math` and `strings` keep explicit
   checks, because arithmetic on a non-number has no operation to attempt;
   theirs became the `Exception`'s message.

   Seven fixtures inverted, as predicted, and no others. They are now
   `math_refuses_with_exception.code`, `strings_refuses_with_exception.code`
   and `net_accepts_what_it_can_render.code`.

   **The guarantee is tiered, and the docs say so.** Rust modules: real —
   panics caught, `runtime_error` deprecated out of the API. C modules:
   policy only — a forgotten NULL check segfaults and an integer `100 / 0`
   raises SIGFPE, neither catchable by anything (both measured). Rust is now
   the recommended path for third-party modules; C stays the ABI's reference
   implementation.
3. ~~**The C runtime gains an error channel**~~ — **shipped 2026-08-28.**

   Of the 33 `code_runtime_error` sites (34 was a miscount), **19 across 16
   functions** are the program's own logic and now go down the channel:
   `add`/`sub`/`mul`/`div`×2/`neg`/`not`/`compare`, `field`, `index`,
   `iter_len`, `check_particle`, `assert`×2, `bool_value`, and
   `core_dispatch`×4. The other 13 stay fatal on purpose — out of memory
   (×3), the `CODE_CHECK_LEAKS` abort, the static-module ABI check, module
   load failure (×5), a module reporting a negative variable count (×2), and
   wasm fractional-number text. Those are the "Still open" list below, not
   this phase.

   **The channel is a flag, not a status return** — decided against the
   status return this document previously leaned toward. Three of the sixteen
   helpers already return a meaningful value (`code_compare` → -1/0/1,
   `code_bool_value` → the bool, `code_iter_len` → the length), so a status
   return would have meant out-parameters and an irregular signature change
   across the set. Instead `fail()` records the message and sets
   `code_failed`; the value-returning three answer 0, which the caller never
   looks at because it checks the flag first.

   **`codegen.rs` checks after every fallible call, through one method.**
   `check_failed` loads `code_failed`, branches to an `unwind` block, and
   continues in `ok` — 12 call sites, which is fewer than 19 because emission
   points are shared (all four arithmetic operators come from one `build_call`).
   Routing every fallible call through one method is what makes "check after
   each one" structural instead of a rule to remember.

   **Behaviour is unchanged, deliberately.** Every `unwind` block ends in
   `code_abort_failure`, which forwards to `code_runtime_error` — same
   message, same exit 1, same wasm reporting path. What moved is *where* the
   program dies: inside the generated function, where a `HandlerFrame` is in
   scope. Phase 4 replaces two lines in `unwind`.

   The existing suite covers this without additions, and a mutation test
   proved it rather than assuming: deleting the `code_compare` check alone
   makes `fail_comparison_type_mismatch.code` and `fail_string_ordering.code`
   exit 0. The three checks that would otherwise fail *silently* —
   `compare` (0 reads as "equal"), `bool_value` (0 reads as `false`),
   `iter_len` (0 reads as "empty, iterate nothing") — are each pinned by a
   fixture that already existed.

   **`code_bool_value` and `code_assert` left the module ABI.** Not renamed —
   removed from `code_abi.h` and from `code-native`, where they were
   `bool_value`/`assert_value`. Nothing in the tree called them, both are the
   compiler's own, and after this phase they report through a flag that only
   the *host's* generated code reads: a `.so` carries its own copy of the
   runtime, so a module calling one would have had its failure silently
   swallowed. `code_field` and `code_index` have the same hazard and stayed,
   because modules can legitimately want them — both now carry the warning in
   the header, and `code-native`'s `field` doc, which claimed a non-Object
   writes Null, was corrected to say it fails.
4. ~~**`assert` returns `Exception`**~~ and
5. ~~**Every other runtime error returns `Exception`**~~ — **both shipped
   2026-08-28, as one change.** They were listed as two phases on the
   assumption that `assert` needed its own mechanism. It does not: every
   runtime failure already travelled phase 3's channel, so making the landing
   block frame-aware delivered all of them at once. `assert`, arithmetic,
   division by zero, field access, `loop` operands, ordering, non-particle
   `emit` — one behaviour, one code path.

   **The landing block now asks whether it is in a frame.** Inside a handler:
   `code_take_failure` builds the Exception into `frame.out` and branches to
   `frame.exit`, which is the block that already clears the re-entry guard and
   releases the invocation's slots — so a failing handler cleans up exactly as
   a returning one does. At the top level there is no frame, and
   `code_abort_failure` ends the program with a non-zero status, which is what
   `return Exception` from the outermost call means anyway.

   The interpreter needed one arm: `Err(e) => Ok(exception(e))` at
   `dispatch_handler`'s boundary. Everything nested — an arithmetic error deep
   in an `if` inside a `loop` — already propagated there as `Err`.

   **A returned Exception does not keep propagating.** Only the frame where
   the failure happened unwinds; the caller gets an ordinary value and may
   ignore it, inspect it, or pass it on
   (`handler_failure_does_not_unwind_the_caller.code`).

   **codegen no longer calls `code_runtime_error` anywhere.** The declaration
   is gone from `Gen`. Every language-level failure goes through the channel;
   `runtime.c` keeps the function for `code_abort_failure` and the 13
   deliberately-fatal sites.

   Two decisions came out of the four inverted fixtures, both settled by the
   owner on 2026-08-28:

   - **A runtime handler cycle answers with an Exception** rather than
     aborting. It is the *emit's* failure, so the frame that tried to
     re-enter gets it back and the invocation already running is untouched —
     which is why the re-entered function returns straight out instead of
     branching to `exit`, since `exit` would clear a guard belonging to a
     frame further down the stack. `handlers::check_cycles` still refuses the
     statically visible cycles before either backend runs.
     `fail_handler_dynamic_cycle.code` → `handler_dynamic_cycle_is_exception.code`.
   - **`verify_defined` moved to `src/verify.rs` and now runs in both
     backends.** It was codegen-only, which made an undefined name a
     compile-time error under `code build` and a runtime error under `code
     run`. Untidy but harmless while both ended the program — and then phase 4
     made a handler body's runtime errors into values, so `code run` would
     have completed a program `code build` refused. That is the one divergence
     the two modes may not have, and the fixture harness caught it. An error
     before the program starts is acceptable, and better than one halfway
     through, after side effects.

   `fail_handler_return_non_particle` and `fail_handler_return_plain_object`
   became `handler_return_*_is_exception.code`: a handler whose result is not
   a particle has failed, and a failed frame answers. Three fixtures were
   added — `handler_assert_failure_is_exception.code`,
   `handler_failure_does_not_unwind_the_caller.code`, and
   `handler_runtime_errors_are_exceptions.code` (one case per failing helper
   family).

   **New: error message text is now a value the program can read**, via
   `Exception.message`. The two backends do not always agree on it — `1 + "a"`
   is *"cannot apply Add to number and string"* interpreted and *"cannot apply
   '+' to these values"* compiled. Where they do agree the fixtures assert the
   message in full, precisely because the harness only compares pass/fail and
   could not otherwise catch a shape or wording that drifted. Unifying the
   wording is now a correctness matter rather than a cosmetic one — see "Still
   open".

## Still open

- **The two backends word the same error differently, and that is now
  observable.** `Exception.message` is an ordinary string a program can read,
  compare, or print, so *"cannot apply Add to number and string"* versus
  *"cannot apply '+' to these values"* is a real difference in what a program
  computes, not just in what a user sees on stderr. The fixture harness cannot
  catch it (pass/fail only), and neither can a fixture that asserts a message,
  since it would simply fail in one mode. `runtime.c` already carries "must
  match interpreter.rs exactly" comments for `type_name` and `article_for`;
  the operator messages need the same treatment. Nothing depends on it yet.
- **`code_field`/`code_index` still owe modules an answer.** They are the two
  fallible helpers left in the module ABI, and a failure raised inside a `.so`
  sets that copy's flag where nobody reads it. Warned about in `code_abi.h`
  rather than fixed.
- **A failing `assert` at the top level ends the program**, which is correct
  (non-zero exit is what `return Exception` means from the outermost call) but
  means the top level is the one place an error is not a value. Left as is,
  deliberately.
- **Out of memory, wasm fractional number text, the `CODE_CHECK_LEAKS`
  abort, and a native module failing to load** — noted 2026-08-28 to be
  discussed one at a time; every case that ends a program with an error is
  to be revisited.

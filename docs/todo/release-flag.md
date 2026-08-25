# `code build --release`: opt level is hardcoded, dev builds pay -O2

The old CLI took `--release` and switched the LLVM opt level accordingly —
default `-O0` for fast iteration, `--release` for `-O2`
(`old/src/main.rs:47,52`; `old/src/codegen.rs:107–110`). The rewrite
inverted the default: `OptimizationLevel::Default` (-O2) is hardcoded at
`src/codegen.rs:595` with no flag anywhere in `src/main.rs`. So today every
build — including throwaway experiments — pays full optimization, and there
is no knob either way.

Not a regression in output quality (everything is optimized now); a lost
dev-loop speed. Builds of nontrivial programs take visibly longer than they
need to during iteration.

## Fix direction

Small and mechanical:

1. `compile_to_object` (`src/codegen.rs:263`) gains a
   `release: bool` parameter; line 595 becomes
   `if release { OptimizationLevel::Default } else { OptimizationLevel::None }`.
   Callers: `src/lib.rs:119` (thread it through whatever wraps
   `compile_to_object`) and the unit-test call at `src/codegen.rs:1870`
   (pass `true` — tests want the real pipeline).
2. `src/main.rs`: accept `--release` in the `build` argument loop (~line
   49, beside `-o`/`--target`), thread it into `build_file` (line 134) →
   the lib entry point. Update the usage string (line 111) and `USAGE`
   (line 117).
3. Decide the default. Two defensible options:
   - **Match old:** default `-O0`, `--release` for -O2. Fastest dev loop;
     shipped artifacts must remember the flag.
   - **Keep current behavior as default:** default -O2, add `--no-release`
     (or `--debug`) for -O0. Safer for people who `build` and ship in one
     step; dev loop stays slow unless opted in.
   The owner called the old behavior the reference, so lean option 1 — but
   it changes what everyone's existing muscle memory produces, so confirm
   before implementing.
4. Fixture coverage: nothing behavioral changes (same IR modulo opts), so
   no new `.code` fixtures; a `tests/build_targets.rs` assertion that
   `--release` and default produce different object sizes would guard the
   flag from silently doing nothing.

Cheapest item on the missing-feature list — an afternoon including the
decision meeting.

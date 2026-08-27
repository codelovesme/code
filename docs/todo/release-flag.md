# `code build --release`: opt level is hardcoded, dev builds pay -O2

> **Shipped 2026-08-27.** `code build` is unoptimized by default and takes
> `--release` for `-O2`, matching the old CLI. What follows is the original
> write-up; "What shipped" at the bottom records the decision and where the
> code landed.

The old CLI took `--release` and switched the LLVM opt level accordingly —
default `-O0` for fast iteration, `--release` for `-O2`
(`old/src/main.rs:47,52`; `old/src/codegen.rs:107–110`). The rewrite
inverted the default: `OptimizationLevel::Default` (-O2) was hardcoded in
`compile_to_object` with no flag anywhere in `src/main.rs`. So every build —
including throwaway experiments — paid full optimization, and there was no
knob either way.

Not a regression in output quality (everything was optimized); a lost
dev-loop speed. Builds of nontrivial programs took visibly longer than they
needed to during iteration.

## What shipped

**The decision: option 1 — match `old/`.** Default `-O0`, `--release` for
`-O2`. The owner called the old behavior the reference, and `code build` is
the inner loop of writing a program: paying `-O2` on every throwaway build
costs more than it buys. The cost of the choice is real and worth stating —
someone who builds and ships in one step now ships an unoptimized artifact
unless they remember the flag. That is the same bargain `cargo` makes.

The plumbing, in the order the flag travels:

1. `src/main.rs` — `--release` in the `build` argument loop beside
   `-o`/`--target`, threaded into `build_file` → `code::compile_file`. Both
   the inline usage string and `USAGE` list it.
2. `src/lib.rs` — `compile_file` gained a `release: bool`, passed straight
   to `codegen::compile_to_object`.
3. `src/codegen.rs` — `compile_to_object` gained the same parameter and
   picks `OptimizationLevel::Default` or `::None` at
   `create_target_machine`. That call site is the only consumer.

Nothing else changed: `cc` is invoked with no `-O` flag at all (it never
was), so the C runtime compiles the same way regardless, and the flag has no
effect on what a program means.

### How much this was actually costing

Measured on an 8000-line generated program (4000 `let` + 4000 `assert`
pairs), building an `Exe`:

| | wall time | artifact |
|---|---|---|
| default (`-O0`) | **2.9 s** | 2 095 640 bytes |
| `--release` (`-O2`) | **121.6 s** | 2 058 776 bytes |

A 41× difference in build time for a 1.8% smaller artifact. The fixture
corpus is far too small to show this — every `tests/*.code` file builds in
under 0.2 s either way, which is why the cost went unnoticed — but the gap
opens fast with program size, so the flag matters exactly for the programs
someone would be iterating on.

### Coverage

`tests/build_targets.rs::release_optimizes_and_the_default_does_not`. The
flag is the easy one to break silently — drop it anywhere along the three
hops above and every other test still passes — so the test compiles one
fixture twice and asserts the two objects differ, runs the optimized one to
confirm `-O2` did not change the program's meaning (the fixture,
`object_merge.code`, is a wall of asserts), and spawns the real CLI with
`--release` so the argument parsing is covered too. Verified by mutation:
hardcoding the opt level again makes it fail, and only it.

**Compare objects, by content — not linked artifacts, by size.** The first
version of this test compared the *sizes* of two linked executables. It
passed locally (56888 vs 52792 bytes) and failed on CI, where the linker's
section padding rounded both to exactly 56728 — the objects underneath had
differed the whole time. Size is a proxy for "the optimizer did something",
and a linker is free to absorb it; `compile_to_object` has nothing between
it and the flag.

The rest of the suite builds at the new default, which means `-O2` is no
longer exercised across the whole fixture corpus the way it was when it was
hardcoded — that test is now the only `-O2` execution coverage. Worth
remembering if an optimizer-triggered bug ever shows up in the wild.

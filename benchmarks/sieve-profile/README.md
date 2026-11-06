# Guarded Bool part writes (234)

The website and public-release evidence remain frozen. This is a source-level
before/after experiment, not a replacement for those language comparisons.
Baseline source is e742d5b, Code v2.23.1; candidate changes only codegen's
existing guarded part-write path to also accept a plain Bool inside versioned
loops. No compact storage, ABI, runtime C, interpreter or language change.

## Profile and proposed change

Matched current-source Callgrind shows the million-bound sieve spends 37.98%
of its instructions in code_set_at and 8.94% in own_level. Known numeric writes
already have an inline guarded path; known Bool writes did not. The candidate
uses the same path/ownership/bounds/shape guards, requires an existing Bool
tag, and stores only the i32 Bool payload. Any mismatch branches to the
original generic statement. Non-loop and boxed/dynamic Bool writes retain the
original path. Instruction shares are not wall-time shares.

Local diagnostics include construction-only, construction+scan,
construction+mark, full sieve, and a numeric-flag diagnostic. Probe totals
include initialization/cleanup and are not additive isolated phase timings.
Massif logical heap totals are separate from process RSS: generic value slots
and temporary overlapping growth buffers contribute to memory. This experiment
does not compact flags or claim lower process memory.

## Fixed validation and timing protocol

- Full-program assertion checks, unchanged sources, 17 workloads: both sizes
  of matrix/sieve/merge from 233, five broader native workloads, scalar loop,
  retained matrix snapshots and all four history-pilot cases.
- Both compilers are release workspace builds from the same hosted toolchain;
  their identities and the exact baseline/candidate revisions are recorded.
- 15 seeded shuffled serial rounds, 34 entries, 510 retained samples on one
  allowed logical CPU of a fresh Ubuntu VM. Compiler/profiler/test processes
  must finish before timing. All binaries are built/checked before sampling.
- Startup, input construction, algorithm, complete checks and cleanup included;
  compiler/harness/wrapper startup excluded. No dropped samples. Retain
  medians/full ranges/MAD and every source/native/wrapper hash.
- Keep only with at least 10% lower million-bound sieve median. A broader
  runtime regression is material if >=10% and its absolute increase exceeds
  max(0.2 ms, 3×before MAD, 3×after MAD). Record every change, including
  smaller/uncertain differences. RSS is process peak, not heap; no allocation
  or dedicated-hardware/fixed-frequency guarantee.
- Preserve native interpreted/dev/release and compiled wasm dev/release
  semantics, aliases, mixed-value/heap-release fallback, shape/new-field
  fallback and negative/fractional/out-of-range errors. Run full workspace,
  formatting/clippy, wasm interpreter/examples and application parity checks.

Local preparation collects no timing samples:

```
PYTHONDONTWRITEBYTECODE=1 python3 benchmarks/sieve-profile/run.py \
  --before /path/to/frozen-before --after /path/to/frozen-after \
  --output-dir /tmp/new-bool-smoke --smoke-only
```

Timing is restricted to the manual `Measure guarded Bool part writes` workflow.
Verify retained results with `verify.py RESULTS [--artifacts DOWNLOADED_DIR]`.

# Compiled algorithms — preregistered study (233)

Six fixed cases, with public Code v2.23.0, GCC O2 and Rust O3 in each:

| Algorithm | Sizes | Result validation |
|---|---|---|
| Dense matrix multiplication | 64×64, 128×128 | Every output against the analytic sum, absolute tolerance 1e-8 |
| Eratosthenes sieve | Inclusive bounds 100,000 and 1,000,000 | Prime count and sum against independent trial division |
| Bottom-up stable merge sort | 16,384 and 65,536 values | Every output equals its index (input is a permutation) |

The cases/sizes were recorded in tracker 233 before measurement. This is a
native complete-program comparison, not a claim of general language speed.
No result-dependent exclusions, algorithm substitutions or favorable-only
publication. All 18 entries and 270 raw samples stay in the final report.
The website and earlier datasets are unchanged; a separate hosted VM means
these numbers cannot be pasted into their old-host rankings.

## Algorithm and representation boundaries

- All matrix programs store flat row-major arrays, initialize
  A[i,j]=(i+j+1)/n and B[i,j]=(2i+j+1)/n, then use identical i/k/j traversal.
  The inner loop hoists A[i,k] once and accumulates into C. Every cell is
  checked against the closed form using sums of k and k².
- All sieves initialize flags, start marking at p² through the inclusive
  limit, then scan all flags for the prime count and sum. Expected values
  come from Python trial division before targets are built, outside timing.
- All sort inputs use the same odd-step permutation: start at 17, add 8191,
  subtract n on wrap. Sizes are powers of two, so every integer 0…n−1 occurs
  once. All implement iterative width-doubling stable merge sort, a shared
  left-first tie rule, the same leftover-copy loops and two reusable buffers.
  No standard-library sort, BLAS, specialized sieve or external module.
- C/Rust matrix and sort use double/f64; Code numbers use the language's
  general number values in generic arrays. C bool and Rust Vec<bool>
  (byte-sized bool elements, not C++ vector<bool>) provide typed flags;
  Code uses generic Bool values. Native index widths/checks, allocation,
  typed/generic storage and compiler optimizations differ. Code buffer swaps
  use assignments and release their temporary alias immediately. C swaps
  pointers; Rust swaps Vec handles. No manual sharing workload is involved.
- Initialization, the algorithm, result checks, startup and cleanup are timed;
  compilation and the Python oracle/harness are excluded. C/Rust may optimize
  known-size loops/zero initialization differently. These are straightforward
  matching algorithms, not equally tuned assembly or isolated kernel timings.
  No fast-math flags, output suppression tricks, unsafe Rust or unchecked
  indexing. Assertion checks remain enabled in every implementation.

## Reproduction and measurement

```
PYTHONDONTWRITEBYTECODE=1 python3 benchmarks/compiled-algorithms/run.py \
  --code /path/to/public-v2.23.0/code --output-dir /tmp/new-algorithm-smoke --smoke-only
```

The launcher and driver must match the actual public release SHA-256s; a
matching version string alone is insufficient. This release is deliberately
the same version as the existing comparison, not a development binary.
Local smoke builds/runs all 18 programs without collecting timing samples.

Publication timing runs only through the manual `Measure compiled algorithm
benchmarks` workflow on a fresh Ubuntu 24.04 hosted VM. GCC uses -O2
-march=native; rustc uses opt-level=3/target-cpu=native, without Cargo startup.
Code uses `build --release` from the published bundle. All native artifacts
are built and checked before sampling. The sampler pins to one allowed CPU,
runs 15 seeded shuffled rounds serially, checks for concurrent compiler/test/
profiler processes and retains every sample, median/min/max/MAD and zero-RSS
count. OS caches are not flushed; shared physical hosts/fixed CPU frequency
are not guaranteed.

The existing fork/exec/wait4 wrapper excludes Python/wrapper startup. RSS is
the largest process resident peak, not live heap, allocation count or a summed
process-tree footprint. Small programs can sit near startup/accounting floors.
Exact source/binary/tool hashes, versions, runner image, CPU/affinity/pressure
and timing boundaries are recorded. Use `verify.py RESULTS [--artifacts DIR]`
to independently recompute every summary and verify downloaded artifacts.

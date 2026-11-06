# One-walk UTF-8 text slices — preregistered source experiment

Task 238 follows the [document-history diagnosis](../document-profile/README.md).
Frozen source baseline `6540a56a00abd41ff2a028769f2a42ded4298d24` includes
the independent vacuole merge. Both source compilers identify as v2.23.2;
neither a version string nor these source measurements imply a new release.

The candidate changes only finite-bound string slicing in `runtime.c` and its
identical vendored copy. It locates both character boundaries in one forward
walk, stopping at the terminator or requested upper bound. Bounds saturate to
the machine index range before conversion. Array slices, non-finite text-bound
handling, `code_length_of`, compiler, interpreter, ABI and value layout stay
unchanged. No character cache, byte indexing or new syntax is introduced.

## Fixed protocol and gate

The manual `Measure one-walk text slices` workflow / branch push builds the
baseline and candidate with identical LLVM 17/Rust toolchains and **separate
Cargo output trees**, then prepares and checks every executable before sampling.
Fresh hosted VM, one allowed logical CPU, serial seeded shuffled order, 15
launches per mode/case: **17 workloads, 34 entries, 510 samples**, none removed.
No concurrent compiler/test/profiler during sampling. Physical CPU isolation
and fixed frequency are not guaranteed.

Cases are exactly the unchanged task-234 source suite: matrix 64/128, sieve
100000/1000000, merge 16384/65536, binary trees, dictionary, point records,
spectral norm, string building, numeric sum, retained snapshots, configuration
4 KiB/64 KiB and document 64 KiB/1 MiB. Complete construction, computation,
original result checks and cleanup remain. No validation is removed from the
document programs. Process startup is included; source compilation, Python and
measurement-wrapper startup are excluded. OS caches remain warm.

Retain only if full document-1m median elapsed time improves **at least 10%**
and no other case materially regresses: **at least 10% slower AND an absolute
increase greater than max(0.2 ms, 3×before MAD, 3×after MAD)**. Otherwise revert
the candidate and retain the investigation. Small accepted costs remain visible.
RSS is largest process-resident peak from `wait4`, not useful heap, allocation
count or a summed process tree; lowering scan work does not imply lower memory.
Keep every raw sample, range, median, MAD, zero-RSS count and source/tool/native
artifact identity. Independently verify statistics and downloaded artifact bytes.

Local functional preparation is untimed:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 benchmarks/document-slice/run.py \
  --before /path/to/frozen-source-before \
  --after /path/to/frozen-source-after \
  --output-dir /tmp/new-slice-smoke --smoke-only
```

Timing requires a fresh hosted workflow and clean tracked checkout. Never mix
these source before/after numbers with public-release website ranks or infer
C/Rust standings from them. The website and historical protocols are frozen.

## Correctness requirements

The new fixture compares 768 empty/ASCII/multibyte slices with an independent
character-list oracle, including fractions and bounds exceeding 32/64-bit
integer ranges. It also covers combining scalars, emoji, all interval forms,
infinity fallback, self assignment, nested aliases and independent snapshots.
Native interpreted/dev/release and compiled-WASM dev/release paths must agree;
native execution includes runtime leak checks. Wrong-kind start/end diagnostics
are checked by message parity. NaN continues through the untouched legacy
path; its pre-existing conversion behavior is outside this finite-bound change.

A retained implementation additionally needs the full workspace, native vendor
sync, clippy/formatting, applicable native application fixtures, source-built
WASM/gallery checks, local matched-source profiles, and Aquarium WASM size.
More than 5% Aquarium growth needs explanation. Both PR CI jobs must pass before
merging. No tag/release, website update or live application restart is part of
this experiment.


## Retained outcome

[All measured outcomes](RESULTS.md): document-1m takes 273.388 → 147.846 ms
(45.92% less time / 1.85× as fast), with peak RSS unchanged. All 510 samples
and small costs are retained. Both PR CI jobs pass and PR 79 merged as
5e231d4. These frozen before/after source builds identify as 2.23.2. The
independent main version bump to 2.24.0 is preserved and is not substituted
into this experiment. Reproduction uses baseline 6540a56 and candidate
1dc4c65 (the retained utf8-slice-walk-238 branch), not a newer main build.

Verify the retained source-experiment data:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 benchmarks/document-slice/verify.py \
  benchmarks/document-slice/results/2026-11-06-source \
  --artifacts /path/to/downloaded-utf8-slice-walk
```

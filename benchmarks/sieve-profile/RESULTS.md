# Guarded Bool part writes — measured source improvement

Measured 5 November 2026, 23:51:48–23:52:20 UTC (6 November locally), on a fresh
hosted VM: [run 42132287441](https://github.com/codelovesme/code/actions/runs/42132287441).
Baseline source `e742d5ba4c59dc4b88c4a90351cb72d8ae31ca4f`; candidate source `0d715632e9384a0f3f7c5be55892b2dca1de502a`.
Both identify as Code v2.23.1; this is a source experiment, not a new release.

[Protocol/source](README.md) · [Raw CSV](results/2026-11-05-source/samples.csv) · [Full identities/report](results/2026-11-05-source/report.json) · [Local profiles](local-profile/report.json).

## Result and decision

The unchanged million-bound sieve takes **60.153 → 30.981 ms**:
**48.50% less elapsed time / 1.94× as fast**. Peak process RSS is
**16.656 → 16.656 MiB**; this is a speed improvement, not a memory-layout change.
The 100,000-bound sieve is also faster (5.491 → 3.219 ms, 41.38% less time).

All 17 cases / 34 entries / 510 samples are retained. The other 15 medians
range from −0.87% to +1.80%; the largest small slowdown is configuration-4k,
0.933 → 0.950 ms. Retained snapshots take 299.599 → 303.374 ms (+1.26%).
No case crosses the preregistered material-regression gate: >=10% slower and
an absolute increase exceeding max(0.2 ms, 3×before MAD, 3×after MAD). The target
clears its >=10% gain gate. **Retain the narrow change.** This gate accepts
small costs; it is not proof that every program is unchanged or faster.

11 of the 17 before/after native artifacts are byte-identical on the measured
runner: matrix_64, matrix_128, merge_16384, merge_65536, binary_trees, dictionary, points, spectral_norm, string_build, sum_squares, snapshots. Those median differences are execution variability,
not newly generated instructions. Changed programs and every measured range
remain below. No outliers or unfavorable entries were removed.

These numbers must not be combined with the older public-v2.23.0 charts or
233 C/Rust rankings from other hosts. This run contains no new C/Rust timing
comparison and establishes no general language-wide speed claim. The website
and its public-release evidence remain unchanged.

## Why this change

The sieve writes Bool array elements about 2.12 million times. Before, known
numeric part writes used guarded inline stores, but plain Bool writes always
boxed keys/values and called the general setter. The existing path now handles
plain Bool only inside versioned loops: every container must be unique and of
the expected type/shape, each index valid, and the old destination already Bool.
Only the i32 payload changes. A miss resumes the original generic statement,
which handles shared values, heap releases, new fields, other types and errors.
Straight-line and boxed Bool writes retain their original path. No runtime C,
ABI, value storage, interpreter or language feature changed.

Matched local Callgrind instructions: **474,981,368 → 187,504,221 (60.52% fewer)**.
Before: code_set_at 37.98%, own_level 8.94% of self instructions. These are not
wall-time percentages. Diagnostic numeric flags already take 179,734,229
instructions using the existing numeric fast path, supporting this choice
without changing the actual benchmark inputs.

Local construction-only / construction+scan / construction+mark probes take
91,294,588 / 139,294,746 / 429,746,128 instructions. Their common initialization,
validation and cleanup mean these are not additive isolated phase timings.
Full raw counters and generated diagnostic sources are retained.

Massif also separates memory from setter overhead: Code's logical useful heap
peak is 25,183,104 bytes, the typed C allocation control 1,000,001 bytes. Generic
16-byte slots and overlapping old/new capacity-growth buffers contribute.
These logical heap figures are not process RSS. The C control binary is from
233 and is used only for allocation-layout context, not a same-host speed
ranking. This patch does not compact flags; target RSS staying equal is expected.

## Complete measured outcomes

15 fresh launches per entry. Medians and minimum–maximum, milliseconds.
Startup, construction, algorithm, complete result checks and cleanup included;
compilation/Python/wrapper startup excluded. OS caches remain warm.

| Workload | Before ms (range) | After ms (range) | Time change |
|---|---:|---:|---:|
| matrix_64 | 1.954 (1.881–2.002) | 1.937 (1.870–2.111) | -0.87% |
| matrix_128 | 10.293 (10.209–10.404) | 10.306 (10.232–10.406) | +0.13% |
| sieve_100000 | 5.491 (5.395–5.740) | 3.219 (3.157–3.303) | -41.38% |
| sieve_1000000 | 60.153 (57.387–67.205) | 30.981 (30.042–31.954) | -48.50% |
| merge_16384 | 3.922 (3.818–4.003) | 3.918 (3.854–3.991) | -0.11% |
| merge_65536 | 15.153 (14.961–15.745) | 15.134 (14.963–15.530) | -0.12% |
| binary_trees | 9.923 (9.833–10.256) | 9.953 (9.783–10.295) | +0.31% |
| dictionary | 6.040 (5.984–7.247) | 6.044 (5.970–6.131) | +0.07% |
| points | 1.763 (1.715–1.791) | 1.778 (1.677–1.826) | +0.86% |
| spectral_norm | 133.351 (132.808–134.974) | 133.416 (132.809–134.904) | +0.05% |
| string_build | 1.787 (1.746–1.846) | 1.809 (1.720–1.838) | +1.22% |
| sum_squares | 7.317 (7.260–7.374) | 7.335 (7.248–7.397) | +0.25% |
| snapshots | 299.599 (287.644–306.829) | 303.374 (292.292–311.620) | +1.26% |
| config_4k | 0.933 (0.876–0.995) | 0.950 (0.887–1.157) | +1.80% |
| config_64k | 6.162 (6.106–6.267) | 6.163 (6.104–6.220) | +0.02% |
| document_64k | 26.021 (25.738–26.611) | 25.913 (25.745–26.351) | -0.41% |
| document_1m | 392.858 (390.328–397.005) | 392.234 (389.258–394.349) | -0.16% |

Process peak RSS medians and minimum–maximum, MiB. This is neither live heap
nor allocation count. Small differences can reflect process accounting.

| Workload | Before MiB (range) | After MiB (range) |
|---|---:|---:|
| matrix_64 | 1.734 (1.730–1.734) | 1.734 (1.730–1.734) |
| matrix_128 | 2.172 (2.168–2.227) | 2.172 (2.172–2.195) |
| sieve_100000 | 2.906 (2.902–2.906) | 2.906 (2.906–2.906) |
| sieve_1000000 | 16.656 (16.656–16.656) | 16.656 (16.656–16.656) |
| merge_16384 | 2.109 (2.105–2.109) | 2.109 (2.094–2.109) |
| merge_65536 | 3.402 (3.395–3.457) | 3.402 (3.402–3.414) |
| binary_trees | 2.922 (2.918–2.922) | 2.922 (2.922–2.922) |
| dictionary | 1.543 (1.543–1.543) | 1.543 (1.539–1.543) |
| points | 1.609 (1.605–1.672) | 1.609 (1.605–1.609) |
| spectral_norm | 1.484 (1.484–1.484) | 1.484 (1.480–1.484) |
| string_build | 1.605 (1.602–1.605) | 1.605 (1.605–1.605) |
| sum_squares | 1.355 (1.352–1.355) | 1.355 (1.352–1.355) |
| snapshots | 13.984 (13.969–13.984) | 13.984 (13.969–13.984) |
| config_4k | 1.484 (1.480–1.484) | 1.484 (1.480–1.484) |
| config_64k | 1.609 (1.605–1.609) | 1.609 (1.605–1.609) |
| document_64k | 5.609 (5.609–5.609) | 5.609 (5.605–5.609) |
| document_1m | 70.344 (70.340–70.348) | 70.348 (70.344–70.348) |

## Validation, provenance and preparation history

Independent verification matched every CSV/JSON row and exact group/sample
index, recomputed all medians/ranges/MAD/zero-RSS counts, and checked all
34 native artifacts, 17 generated sources and measurement-wrapper hashes.
Every generated source exactly matches the locally checked smoke inputs.
All RSS samples are nonzero. Both CLI identities/revisions, tool binaries,
Rust/GCC versions, CPU/OS/runner image, affinity and pressure are in the report.
Both release workspaces use the same LLVM 17/Rust toolchain, separate Cargo
output trees, then all executables are built/checked before serial seeded
sampling pinned to one allowed CPU. No concurrent compilers/profilers/tests
were detected inside the VM. Physical hardware isolation/fixed frequency are
not guaranteed. There are no claims about isolated kernel time or build speed.

Local clean-environment full workspace: **341 passed, zero failed, seven ignored,
zero filtered, 69 summaries**. Clippy with warnings denied, Rust/valid-fixture
formatting, release/debug workspace builds, wasm32 interpreter build and all
177 fresh-engine examples pass. Wasm-only build retains two pre-existing
unused-code warnings (RuntimeModule.drain / module_address); no warning was
suppressed. Deliberate parse-failure fixtures are skipped by the formatter.

The new regression covers unique/shared/nested Bool paths, independent copies,
heap-value replacement, shape/new-field fallback, self-containing values and
negative/fractional/out-of-range errors. It passes interpreted, native dev/
release with native leak checks and allocator perturbation, and compiled wasm
dev/release. All **102 application fixtures / 204 native dev+release checks**
pass in a scratch source/module copy with example configuration. An optional
full interpreter-app pass was stopped when legacy equivalence loops proved
expensive; it is not claimed as complete. The interpreter is unchanged and the
new language regression plus workspace suite cover its matching behavior.
Aquarium's compiled wasm stays byte-identical (892,707 bytes); all 46 generated
site files also stay byte-identical, with no new gallery entry.

Preparation issues are retained rather than counted as initially clean:

- The first new fixture needed explicit Array-of-Any labels for mixed-value
  replacement. An editing replacement accidentally altered one assertion;
  its handler-error assertion also expected source runtime instead of the
  existing core convention. Corrected before the successful focused and
  clean full-suite runs. No interpreter/native/wasm answer disagreement occurred.
- Inherited CARGO_TARGET_DIR made the existing module-identity packaging script
  search the wrong output path. Clearing that test environment restored the
  module test and then the complete clean full suite, without a language fix.
- Scratch application checks found an old compiled host/broken.so helper;
  baseline and candidate both failed the same expected-startup assertion.
  Rebuilding fixture stand-ins inside the scratch copy restored all checks.
- The first hosted job, 42131897435, stopped before any timing sample because
  shared Cargo output across worktrees reused the baseline compiler. The
  before/after identity guard caught it. Separate output trees fixed the
  setup; successful run 42132287441 retains all 510 collected samples.

[PR 76](https://github.com/codelovesme/code/pull/76) merged as
`a94eaecc959ae1f7ab69c7f026839d0b0b848bd0` after both jobs in
[CI 42132296175](https://github.com/codelovesme/code/actions/runs/42132296175)
passed; the remote suite also reports 341 passed / zero failed / seven ignored.
CI could not start MinIO, so the conditional S3 fixture skipped its live-service
part; this is separate from Rust's seven explicitly ignored tests. No release/tag, live
application restart or website publication accompanies this source improvement.

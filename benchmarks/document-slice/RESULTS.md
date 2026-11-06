# One-walk text slices — measured source improvement

Measured 6 November 2026, 09:01:31–09:01:51 UTC, on one fresh Ubuntu VM:
[run 42191622017](https://github.com/codelovesme/code/actions/runs/42191622017).
Baseline `6540a56a00abd41ff2a028769f2a42ded4298d24`; candidate
`1dc4c65fae141a7a17a9ee6859c2fdca661abb83`. Both are source builds identifying
as v2.23.2; these are not newly published packages. No release or website update.

[Protocol](README.md) · [Raw CSV](results/2026-11-06-source/samples.csv) ·
[Full report/identities](results/2026-11-06-source/report.json) ·
[Matched local profiles](local-profile/report.json) · [Validation](validation/summary.json).

## Result and decision

The unchanged complete 1 MiB document-history program takes **273.388 →
147.846 ms: 45.92% less time / 1.85× as fast**. The 64 KiB case takes
**16.022 → 8.164 ms: 49.04% less time / 1.96× as fast**. Document-1m
peak process RSS stays **70.469 → 70.469 MiB**; document-64k stays
**5.734 → 5.734 MiB**. This reduces scanning work, not retained-history memory.

All 17 workloads / 34 entries / 510 samples are retained. No case crosses
preregistered material-regression thresholds (>=10% slowdown AND an absolute
increase > max(0.2 ms, 3×before MAD, 3×after MAD)). The other 15 median
changes range from −2.64% to +4.04%; the largest small cost is points,
1.169 → 1.216 ms (+4.04%). These differences are visible below; they are
not additional established optimization wins. The >=10% target gate passes.
**Retain the finite-bound slice change.** Both required PR CI jobs passed;
[PR 79](https://github.com/codelovesme/code/pull/79) merged as
`5e231d4029c442cb5318c4e22d3f11b918aeab5d`.

The earlier website document-1m number (510.264 ms) is from a different
host and is not the baseline here. Do not describe this as 510 → 148 ms,
mix these source results into public-release charts, or infer C/Rust ranks.
No language-wide speed or memory-management superiority is established.

## What changed and why

The task-237 profile identified redundant complete character scans in
`code_slice`. For finite bounds the runtime now locates both UTF-8 character
boundaries in one forward walk. A short prefix stops early; a tail no longer
first counts the whole input and restarts. Huge finite bounds saturate before
conversion to size_t, and the terminator provides the upper clamp. Allocation
and copying of the selected bytes still occur. `code_length_of` and its
separate scans are unchanged. Array and non-finite-bound paths are unchanged,
as are compiler/interpreter logic, ABI 2, headers and value storage/layout.
NaN still uses the original legacy conversion path; no repair or correctness
claim about that pre-existing behavior is made by this finite-bound change.

Matched local full-program Callgrind instruction counts fall from
**5,126,069,855 → 2,978,551,111 (41.89% fewer)** at 1 MiB and
**321,075,435 → 186,822,527 (41.81% fewer)** at 64 KiB.
These are instruction counts, not elapsed-time percentages. Local profile
counts and hosted timings are separate evidence. Full original result checks,
expected-value preparation, retaining all 65 versions and cleanup remain in
both measured programs. No benchmark source or input was simplified.

## Every measured outcome

15 fresh launches per entry. Medians and full minimum–maximum in milliseconds.
Startup, construction, algorithm, complete checks and cleanup included;
source/native compilation and Python/wrapper startup excluded. OS caches warm.

| Workload | Before ms (range) | After ms (range) | Time change |
|---|---:|---:|---:|
| matrix_64 | 1.265 (1.220–1.319) | 1.243 (1.212–1.352) | -1.72% |
| matrix_128 | 5.524 (5.386–5.736) | 5.532 (5.387–5.673) | +0.13% |
| sieve_100000 | 2.167 (2.047–2.246) | 2.110 (2.044–2.267) | -2.64% |
| sieve_1000000 | 17.248 (16.433–18.484) | 17.502 (16.493–25.960) | +1.48% |
| merge_16384 | 2.564 (2.439–2.658) | 2.509 (2.448–2.836) | -2.12% |
| merge_65536 | 9.014 (8.766–17.821) | 9.010 (8.779–9.446) | -0.04% |
| binary_trees | 6.012 (5.926–6.341) | 6.026 (5.936–6.255) | +0.22% |
| dictionary | 3.614 (3.513–3.921) | 3.561 (3.489–4.164) | -1.45% |
| points | 1.169 (1.147–1.252) | 1.216 (1.148–1.275) | +4.04% |
| spectral_norm | 111.837 (110.583–116.673) | 111.508 (110.492–160.595) | -0.29% |
| string_build | 1.149 (1.108–1.204) | 1.158 (1.137–1.273) | +0.83% |
| sum_squares | 6.547 (6.444–6.849) | 6.499 (6.447–6.832) | -0.72% |
| snapshots | 179.710 (177.151–185.091) | 180.174 (177.200–220.867) | +0.26% |
| config_4k | 0.709 (0.682–0.763) | 0.694 (0.672–0.778) | -2.18% |
| config_64k | 2.767 (2.709–2.892) | 2.763 (2.698–3.828) | -0.14% |
| document_64k | 16.022 (15.441–16.740) | 8.164 (7.990–12.063) | -49.04% |
| document_1m | 273.388 (266.845–292.119) | 147.846 (146.575–176.910) | -45.92% |

Process peak RSS, median and minimum–maximum MiB. Not live heap, allocation
count or a sum of process-tree peaks. Small accounting differences are retained.

| Workload | Before MiB (range) | After MiB (range) |
|---|---:|---:|
| matrix_64 | 1.859 (1.859–1.859) | 1.859 (1.855–1.859) |
| matrix_128 | 2.297 (2.293–2.305) | 2.301 (2.297–2.355) |
| sieve_100000 | 2.906 (2.902–2.906) | 2.906 (2.902–2.906) |
| sieve_1000000 | 16.656 (16.652–16.656) | 16.656 (16.652–16.656) |
| merge_16384 | 2.234 (2.230–2.234) | 2.234 (2.227–2.234) |
| merge_65536 | 3.527 (3.527–3.531) | 3.527 (3.527–3.570) |
| binary_trees | 2.922 (2.922–2.922) | 2.922 (2.918–2.922) |
| dictionary | 1.543 (1.539–1.543) | 1.547 (1.547–1.547) |
| points | 1.734 (1.730–1.734) | 1.734 (1.672–1.734) |
| spectral_norm | 1.609 (1.605–1.609) | 1.609 (1.609–1.609) |
| string_build | 1.730 (1.652–1.730) | 1.730 (1.727–1.730) |
| sum_squares | 1.355 (1.355–1.355) | 1.355 (1.352–1.355) |
| snapshots | 14.109 (14.105–14.109) | 14.109 (14.105–14.109) |
| config_4k | 1.609 (1.609–1.609) | 1.609 (1.605–1.609) |
| config_64k | 1.734 (1.734–1.734) | 1.734 (1.734–1.734) |
| document_64k | 5.734 (5.730–5.734) | 5.734 (5.730–5.734) |
| document_1m | 70.469 (70.465–70.473) | 70.469 (70.465–70.473) |

## Integrity and validation

Independent verification matches all 510 CSV/JSON rows, exact groups and
sample indices, medians/ranges/MAD/zero-RSS counts, all 34 native artifacts,
17 generated sources and the measurement wrapper. All RSS samples are nonzero.
Every hosted generated program matches both the local untimed preparation and
the frozen task-234 source corpus byte-for-byte. Compiler/source/tool identities,
CPU/OS/runner image, affinity and pressure are in the report. Separate Cargo
output directories prevent before/after compiler reuse. No concurrent compiler,
test or profiler was detected during hosted sampling; physical isolation and
fixed frequency are not guaranteed. This is a complete-process benchmark.

Local full workspace: **342 passed, zero failed, seven ignored / 69 summaries**.
Rust formatting, valid Code-fixture formatting, clippy with warnings denied and
workspace dev/release builds pass. Source-built WASM interpreter and 178 actual
gallery/featured/lesson checks pass, plus the exhaustive oracle in that engine.
The wasm-only build retains the two pre-existing unused-code warnings
(RuntimeModule.drain and module_address); none was suppressed.

The independent character-list oracle checks 768 empty/ASCII/Unicode slices,
fractions, negative/reversed/huge bounds exceeding both machine integer ranges,
infinity fallback, combining scalars, emoji and every interval form. It passes
interpreted, native dev/release with leaks/allocator perturbation, compiled-WASM
dev/release, and the source-built WASM interpreter. New wrong-bound-kind message
cases pass parity. All 102 fixtures from the frozen application corpus pass
native dev/release (204 checks) in a fresh scratch copy with example config and
rebuilt fixture helpers. This does not claim validation of newer concurrent
application changes or a complete interpreter-app run; the interpreter is
unchanged and the complete workspace/new oracle cover language parity.

Runtime/vendor copies are identical; ABI headers unchanged. Matched-source
Aquarium WASM is **892,435 → 892,938 bytes**, +503 bytes / **0.056%**,
below the 5% investigation gate. All **46 generated website files** remain
byte-identical and the existing gallery remains **147 examples**.

Preparation corrections, not runtime failures:
- The first fixture draft had two unnecessary line-ending commas. They were
  removed before successful baseline/candidate validation.
- The first local Cargo invocation found the owned target symlink pointed at
  an absent directory. Creating the destination fixed setup before building.
- The exhaustive regression was given the existing stress_ prefix so it stays
  outside the public gallery. The full suite passed before this path-only
  rename; final-name language and compiled-WASM checks passed afterward.
  PR CI exercises the final source tree in full.
- Initial Aquarium output is generated only in the scratch app; no live app
  entry, configuration, bundle or process was changed.

No test failure, runtime crash or mode disagreement occurred after those
preparation corrections. No samples, sizes, workloads or unfavorable outcomes
were excluded. The source improvement is separate from a future package release.


Remote [CI 42191675111](https://github.com/codelovesme/code/actions/runs/42191675111)
also reports **342 passed, zero failed, seven ignored / 69 summaries**. MinIO
was unavailable, so the conditional live S3 fixture portion skipped separately
from those seven ignored tests. The independent main version bump to 2.24.0
was preserved during merge; it did not change runtime/compiler/ABI source.
The numbers above remain the frozen matched 2.23.2 source experiment, not a
measurement of newly published 2.24.0 assets. This patch cuts no release.

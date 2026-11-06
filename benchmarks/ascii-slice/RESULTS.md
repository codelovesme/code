# Rejected: bounded native ASCII slice seeking

**Decision: reject the runtime candidate; main retains the validated 239 code.**
The ASCII target improves, but the large Unicode control fails the performance
gate enrolled before implementation/timing. No runtime or compiler patch is merged.

Measured 6 November 2026, 10:53:21–10:54:00 UTC on one fresh Ubuntu VM:
[run 42205652846](https://github.com/codelovesme/code/actions/runs/42205652846).
Baseline `af3430d26dc7d577feffd756c3e9c444f57eec0b`; rejected candidate
`c0eb1676ebba69b6486d48c3a5e0c40fd03362e6`, both source builds identifying
as 2.24.0. [PR 81](https://github.com/codelovesme/code/pull/81) is closed
unmerged; its branch ascii-slice-seek-240 preserves the experiment for reproduction.

[Protocol](README.md) · [Raw CSV](results/2026-11-06-source/samples.csv) ·
[Complete report/identities](results/2026-11-06-source/report.json) ·
[Eight matched profiles](local-profile/report.json) · [Validation](validation/summary.json).

## Why it failed

The unchanged ASCII document-1m program takes **184.817 → 113.216 ms**,
**38.74% less elapsed time**; ASCII 64 KiB improves **41.49%**. However,
**document_1m_unicode takes 278.960 → 359.838 ms: 28.99% slower**.
That 80.877 ms increase exceeds both the >=10% threshold and the absolute
noise gate max(0.2 ms, 3×before MAD, 3×after MAD) = **3.080 ms**.
The 64 KiB Unicode control is +1.64%. Matrix-128 also costs +6.11%, retained
below even though it does not cross the registered material-regression gate.

The protocol requires >=10% original document-1m gain **and no material cost
in any other case**, including both Unicode controls. Passing the ASCII target
is insufficient. No control, size or unfavorable sample was removed; no
favorable rerun or post-hoc gate change is used to accept the candidate.

Original ASCII document-1m peak RSS stays 70.344 MiB; the Unicode counterpart
stays 141.340 MiB. These are process peaks, not useful heap/allocation counts.
Unicode initial payloads have twice the bytes at equal scalar count. They are
controls for the same history/checking algorithm, not extra public charts.

## What was tried and what remains unknown

The native finite-bound slice path used at most 256 bytes of bounded strnlen
lookahead, never more than the remaining character budget. Verified-readable
ASCII words were skipped via memcpy loads. The first non-ASCII byte resumed
a scalar UTF-8 walk. It assumed no alignment, header cache, storage change,
new ISA or host import. Array/non-finite/error paths and compiled WASM stayed
unchanged. The source branch retains this **rejected** implementation; active
main does not contain its native_slice_offset helper or new runtime calls.

Matched local 1 MiB ASCII instruction counts fall **1,982,412,093 →
704,623,020**; 64 KiB **124,571,419 → 44,692,982**. Unicode-1m counts are
nearly identical, **2,811,754,025 → 2,811,720,139**; Unicode-64k is
**206,488,945 → 206,455,114**. Instruction counts are not CPU cycles or
wall-time shares. Their near equality does **not** establish that the measured
Unicode slowdown is noise. Its exact cause is unproven; no allocator, cache,
code-alignment or hardware explanation is asserted without further evidence.

The study includes startup, construction, all 65 retained versions, edits,
original full checks/expected-value creation and cleanup. Original 17 source
programs are byte-identical to 239. Added Unicode controls change only initial
payload a to é with identical scalar sizes/algorithm. Compilation, Python and
measurement-wrapper startup are excluded; OS caches remain warm. Do not mix
earlier hosts or public release/C/Rust rankings into this paired source study.

## Every measured outcome

19 cases / 38 entries / **570 retained samples**, 15 launches per entry.
Median and full minimum–maximum milliseconds. All cases remain, including losses.

| Workload | Before ms (range) | After ms (range) | Time change |
|---|---:|---:|---:|
| matrix_64 | 2.058 (2.036–2.461) | 2.108 (2.079–2.156) | +2.40% |
| matrix_128 | 10.078 (9.973–10.233) | 10.694 (10.622–10.761) | +6.11% |
| sieve_100000 | 3.210 (3.110–3.310) | 3.171 (3.107–3.298) | -1.21% |
| sieve_1000000 | 26.982 (26.377–28.058) | 26.962 (26.151–28.323) | -0.07% |
| merge_16384 | 4.389 (4.319–4.483) | 4.353 (4.270–4.803) | -0.84% |
| merge_65536 | 16.952 (16.779–17.193) | 16.760 (16.579–18.075) | -1.14% |
| binary_trees | 9.587 (9.514–10.925) | 9.742 (9.637–10.487) | +1.62% |
| dictionary | 6.143 (6.081–6.393) | 6.192 (6.123–6.683) | +0.80% |
| points | 1.885 (1.852–1.974) | 1.893 (1.859–2.118) | +0.43% |
| spectral_norm | 155.195 (154.852–172.385) | 152.269 (151.727–154.623) | -1.89% |
| string_build | 2.027 (1.976–2.114) | 1.971 (1.932–2.050) | -2.75% |
| sum_squares | 10.022 (9.996–10.074) | 10.015 (9.995–10.045) | -0.07% |
| snapshots | 345.249 (344.507–348.121) | 345.641 (344.248–347.697) | +0.11% |
| config_4k | 0.976 (0.953–1.038) | 0.994 (0.969–1.030) | +1.87% |
| config_64k | 4.271 (4.234–4.307) | 4.326 (4.281–4.422) | +1.29% |
| document_64k | 10.705 (10.581–10.837) | 6.263 (6.040–6.455) | -41.49% |
| document_1m | 184.817 (183.656–189.964) | 113.216 (111.803–114.602) | -38.74% |
| document_64k_unicode | 19.413 (19.279–20.071) | 19.732 (19.402–30.320) | +1.64% |
| document_1m_unicode | 278.960 (275.857–282.395) | 359.838 (335.646–363.330) | +28.99% |

Peak process RSS, median and min–max MiB; small accounting differences retained.

| Workload | Before MiB (range) | After MiB (range) |
|---|---:|---:|
| matrix_64 | 1.734 (1.672–1.734) | 1.734 (1.734–1.734) |
| matrix_128 | 2.172 (2.148–2.176) | 2.172 (2.172–2.207) |
| sieve_100000 | 2.844 (2.844–2.844) | 2.844 (2.836–2.844) |
| sieve_1000000 | 16.594 (16.531–16.594) | 16.594 (16.590–16.594) |
| merge_16384 | 2.109 (2.047–2.109) | 2.109 (2.105–2.109) |
| merge_65536 | 3.402 (3.398–3.410) | 3.402 (3.344–3.406) |
| binary_trees | 2.859 (2.855–2.859) | 2.859 (2.848–2.859) |
| dictionary | 1.484 (1.480–1.484) | 1.484 (1.480–1.484) |
| points | 1.609 (1.547–1.609) | 1.609 (1.605–1.609) |
| spectral_norm | 1.488 (1.426–1.488) | 1.488 (1.488–1.488) |
| string_build | 1.605 (1.605–1.605) | 1.605 (1.602–1.605) |
| sum_squares | 1.355 (1.352–1.355) | 1.355 (1.355–1.355) |
| snapshots | 13.984 (13.980–13.984) | 13.984 (13.922–13.984) |
| config_4k | 1.484 (1.484–1.484) | 1.484 (1.473–1.484) |
| config_64k | 1.609 (1.605–1.609) | 1.609 (1.605–1.609) |
| document_64k | 5.609 (5.547–5.609) | 5.609 (5.605–5.609) |
| document_1m | 70.344 (70.340–70.348) | 70.344 (70.340–70.348) |
| document_64k_unicode | 9.984 (9.980–9.984) | 9.984 (9.980–9.984) |
| document_1m_unicode | 141.340 (141.336–141.344) | 141.340 (141.320–141.344) |

## Integrity, correctness and stopping

Independent verification matches every CSV/JSON row, exact groups/sample
indices, all medians/min/max/MAD/zero-RSS counts, 38 native executables, 19
generated programs and the measurement-wrapper hashes. RSS is nonzero in
every sample. All 19 hosted sources match local preparation, and original 17
match the frozen 239 corpus. All eight local raw profiles/source/native
identities are independently checked. Compiler/tool/CPU/OS/runner/affinity/
pressure/source identities are retained. Separate Cargo output trees and
completed preparation prevent reuse/concurrent compilation during serial seeded
sampling. Physical isolation/fixed frequency are not guaranteed.

Candidate full local workspace: **344 passed, zero failed, seven ignored /
69 summaries**. Native interpreter/dev/release and compiled-WASM dev/release
400-slice oracle over an independent 801-scalar list pass, including Unicode
word/chunk transitions, fractions, negative/huge bounds and independent copies.
The actual-runtime guard passes 305,280 cases / 610,560 calls per run with
protected pages, borrowed/owned output aliases and baseline/candidate address/
undefined/leak sanitizers. All 102 frozen app fixtures / 204 native checks,
clippy/format, source-built WASM interpreter and 178 gallery/featured/lesson
examples pass. Those correctness checks do not override a failed speed gate.
Newer app changes and a full interpreted-app run are not claimed.

Aquarium WASM is byte-identical, 892,938 bytes. Matched-basename regression
WASM outputs are also byte-identical in dev/release. All 46 generated site
files stay byte-identical, gallery remains 147 entries. Two pre-existing
wasm-only unused-code warnings (RuntimeModule.drain/module_address) remain.
An initial byte-identity assertion used different embedded output basenames;
matching both to slices.wasm corrected the comparison without changing runtime
code or rerunning timing. No functional test crash or mode disagreement occurred.

After the performance rejection, the unmerged source PR was closed and its
remote CI [42205738512](https://github.com/codelovesme/code/actions/runs/42205738512)
was canceled deliberately. It is **not** reported as green; no runtime merge
followed. Active main retains the 239 implementation. The workflow is archived
as rejected-workflow.yml, not installed as an active workflow on main. The
candidate regression is preserved as regression.code, outside public fixtures.

No release/tag, new language feature, website-content update or live application
restart occurred. The negative result is retained to avoid repeating an already
rejected change without a newly justified investigation.


After rejecting/removing the active experiment, main's unchanged 239 runtime
was verified against the frozen baseline hash and rechecked with the complete
workspace: **343 passed, zero failed, seven ignored / 69 summaries**. Its
previous debug/release source binaries remain current. Only the rejected-study
protocol, harness, archived regression/workflow and evidence are committed;
no candidate runtime, core test or active workflow is retained.

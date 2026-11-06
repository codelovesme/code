# Bounded character counts — measured source improvement

Measured 6 November 2026, 09:45:43–09:46:13 UTC on one fresh Ubuntu VM:
[run 42197158469](https://github.com/codelovesme/code/actions/runs/42197158469).
Baseline `3f9f23c2ba0d442837db86af52a81a5a6ab94978`; candidate
`e6596676cd4c02be7b4ec05c627b8fddf9fd069f`. Both are source builds
identifying as v2.24.0, not newly published packages. The baseline already
includes the retained one-walk slice improvement from 238.

[Protocol](README.md) · [Raw CSV](results/2026-11-06-source/samples.csv) ·
[Full report/identities](results/2026-11-06-source/report.json) ·
[Matched profiles](local-profile/report.json) · [Validation](validation/summary.json).

## Result and decision

The unchanged complete 1 MiB document-history program takes **257.369 →
185.443 ms: 27.95% less elapsed time / 1.39× as fast**. At 64 KiB it takes
**14.778 → 10.682 ms: 27.72% less time / 1.38× as fast**. Peak process RSS
stays **70.344 → 70.344 MiB** and **5.609 → 5.609 MiB**, respectively.
This reduces character-scanning work, with no new cache or storage change.

All 17 cases / 34 entries / 510 samples remain. No case crosses the fixed
material-regression gate: >=10% slower AND absolute increase greater than
max(0.2 ms, 3×before MAD, 3×after MAD). The other 15 median changes range
from −1.03% to +4.10%; the largest small cost is string_build, 1.937 →
2.016 ms (+4.10%). Every smaller cost/range remains below. The >=10% target
gate passes. **Retain the character-count change.** Both required PR CI jobs passed;
[PR 80](https://github.com/codelovesme/code/pull/80) merged as
`5ac7d822b5d690b66ab82cc27d996f3ecfe91a3d`.

The previous 238 after-value (147.846 ms), its before-value (273.388 ms)
and the website figure (510.264 ms) came from other hosts. The baseline for
**this** paired study is 257.369 ms. Cross-host speedup/regression ratios,
compounded earlier ratios and C/Rust rankings are not established. The public
website evidence remains unchanged. No general language-wide speed or memory
management superiority is claimed.

## Why and what changed

The matched post-238 profile placed 40.56% of self instructions in
code_length_of. The string branch now takes its existing NUL-terminated byte
bound with strlen, counts continuation-byte bits in bounded eight-byte memcpy
loads using unsigned arithmetic, and handles the tail scalarly. Subtracting
continuation bytes gives the same scalar count as the old per-byte loop.
Every word load stays before the known first-NUL bound. There is no alignment
cast, header write, new character cache, architecture-specific SIMD requirement
or host import. Array/object/error branches, code_slice, compiler/interpreter
logic, ABI 2 and value/header layout remain unchanged.

Matched local full-program Callgrind counts fall from **2,978,551,111 →
1,982,412,093 (33.44% fewer instructions)** at 1 MiB, and **186,822,527 →
124,571,419 (33.32% fewer)** at 64 KiB. These are counts, not elapsed-time
shares or isolated phase timers. Local instruction diagnostics and hosted
wall-time measurements are separate. In the new 1 MiB profile, code_slice
still accounts for 74.48% of self instructions; this change does not remove
its UTF-8 boundary walks or eliminate text copying.

The original ASCII payloads, all 65 retained histories, expected-value
construction, complete result checks and cleanup remain in both measured
programs. No benchmark program or validation was simplified to obtain a gain.

## Every measured outcome

15 launches per entry. Median and complete minimum–maximum, milliseconds.
Startup, construction, algorithm, all checks and cleanup included; compilation
and Python/measurement-wrapper startup excluded. OS caches remain warm.

| Workload | Before ms (range) | After ms (range) | Time change |
|---|---:|---:|---:|
| matrix_64 | 2.057 (2.019–2.120) | 2.053 (2.023–2.362) | -0.18% |
| matrix_128 | 10.016 (9.981–10.283) | 10.076 (9.992–10.204) | +0.60% |
| sieve_100000 | 3.213 (3.148–3.405) | 3.238 (3.150–3.488) | +0.79% |
| sieve_1000000 | 27.676 (26.603–28.998) | 27.715 (27.135–28.565) | +0.14% |
| merge_16384 | 4.415 (4.367–4.455) | 4.432 (4.330–4.775) | +0.39% |
| merge_65536 | 17.036 (16.867–17.783) | 16.999 (16.780–17.160) | -0.21% |
| binary_trees | 9.720 (9.581–10.472) | 9.670 (9.563–10.013) | -0.52% |
| dictionary | 6.201 (6.102–6.363) | 6.163 (6.070–6.283) | -0.62% |
| points | 1.888 (1.864–1.949) | 1.895 (1.847–1.947) | +0.39% |
| spectral_norm | 155.164 (154.680–156.604) | 155.024 (154.521–155.929) | -0.09% |
| string_build | 1.937 (1.903–1.991) | 2.016 (1.984–2.068) | +4.10% |
| sum_squares | 10.033 (9.991–10.177) | 10.023 (9.989–10.171) | -0.10% |
| snapshots | 344.832 (344.143–345.596) | 345.355 (344.319–349.576) | +0.15% |
| config_4k | 0.992 (0.923–1.033) | 0.982 (0.959–1.053) | -1.03% |
| config_64k | 4.352 (4.319–4.441) | 4.379 (4.302–4.432) | +0.62% |
| document_64k | 14.778 (14.538–15.089) | 10.682 (10.465–10.880) | -27.72% |
| document_1m | 257.369 (256.292–259.520) | 185.443 (183.894–186.777) | -27.95% |

Peak process RSS, median and minimum–maximum MiB. Not live heap, allocation
count or summed process-tree memory. Small accounting differences are retained.

| Workload | Before MiB (range) | After MiB (range) |
|---|---:|---:|
| matrix_64 | 1.734 (1.730–1.734) | 1.734 (1.730–1.734) |
| matrix_128 | 2.172 (2.168–2.176) | 2.172 (2.172–2.176) |
| sieve_100000 | 2.844 (2.840–2.844) | 2.844 (2.844–2.844) |
| sieve_1000000 | 16.594 (16.590–16.594) | 16.594 (16.590–16.594) |
| merge_16384 | 2.109 (2.105–2.109) | 2.109 (2.047–2.109) |
| merge_65536 | 3.402 (3.402–3.465) | 3.406 (3.398–3.406) |
| binary_trees | 2.859 (2.855–2.859) | 2.859 (2.859–2.859) |
| dictionary | 1.484 (1.480–1.484) | 1.484 (1.480–1.484) |
| points | 1.609 (1.547–1.609) | 1.609 (1.605–1.609) |
| spectral_norm | 1.484 (1.484–1.484) | 1.488 (1.484–1.488) |
| string_build | 1.605 (1.602–1.605) | 1.605 (1.602–1.605) |
| sum_squares | 1.355 (1.355–1.355) | 1.355 (1.352–1.355) |
| snapshots | 13.984 (13.973–13.984) | 13.984 (13.984–13.984) |
| config_4k | 1.484 (1.484–1.484) | 1.484 (1.480–1.484) |
| config_64k | 1.609 (1.605–1.609) | 1.609 (1.609–1.609) |
| document_64k | 5.609 (5.605–5.609) | 5.609 (5.605–5.609) |
| document_1m | 70.344 (70.340–70.348) | 70.344 (70.340–70.348) |

## Integrity, safety and validation

Independent verification matches all 510 CSV/JSON rows and exact sample/group
indices, medians/min/max/MAD/zero-RSS counts, all 34 native executable hashes,
17 generated sources and the measurement-wrapper bytes. All RSS samples are
nonzero. Each hosted source matches the local untimed preparation and frozen
238 corpus byte-for-byte. Four matched local profile/source/native identities
and raw summaries are independently verified.

Exact compiler/source/tool identities, Ubuntu/CPU/runner image, affinity and
pressure are in the report. Separate Cargo output trees prevent compiler
reuse across source worktrees; preparation and checks finish before serial
seeded shuffled sampling. No concurrent compiler, test or profiler was detected
during hosted timing. Physical isolation and fixed frequency are not promised.

Local full workspace: **343 passed, zero failed, seven ignored / 69 summaries**.
Rust and valid Code-fixture formatting, workspace dev/release builds and clippy
with warnings denied pass. The source-built WASM interpreter runs all 178
gallery/featured/lesson examples and the new Unicode-count regression. Its
wasm-only build retains two existing unused-code warnings (RuntimeModule.drain
and module_address); none was suppressed.

The new language regression independently counts 97 Unicode snapshots while
growing text, using a unique sentinel to expose incorrect lengths. It covers
every byte-tail size, emoji, combining scalars, literal/owned/shared/grown/
sliced/empty values and an unchanged array case. Interpreter/native dev and
release with leak checks/allocator perturbation, compiled-WASM dev/release and
source-built WASM interpreter pass. Wrong-kind length diagnostics pass parity.

The actual-runtime C guard checks **66,048 cases / 132,096 length calls per
run**: all byte values 1..255 at lengths 0..256, mixed bytes, first embedded
NUL, every word-tail/alignment, borrowed text and an owned output/input alias.
The NUL is the final readable byte before a protected page. Candidate normal
and baseline/candidate address/undefined/leak-sanitized runs pass with immediate
sanitizer failure enabled. This preserves legacy byte-count behavior even for
arbitrary nonzero byte patterns; it introduces no UTF-8 validation feature.

All **102 fixtures / 204 native dev+release checks** from the frozen application
corpus pass in a fresh scratch copy using example configuration and rebuilt
fixture helpers. Newer concurrent app changes and a complete interpreted-app
run are outside that claim; the interpreter is unchanged and the workspace/
new regression cover language parity. No live application was changed.

Runtime/vendor copies are identical, ABI headers unchanged. Matched-source
Aquarium WASM is **892,938 bytes before and after, byte-identical**. All **46
generated website files** also remain byte-identical; gallery stays **147
examples** because the exhaustive regression uses the existing stress_ prefix.

No preparation/test failure, runtime crash or mode disagreement occurred.
No sample, size, workload or unfavorable outcome was removed. No release/tag,
website-content change or live application restart accompanies the experiment.


Remote [CI 42197207277](https://github.com/codelovesme/code/actions/runs/42197207277)
also reports **343 passed, zero failed, seven ignored / 69 summaries**. MinIO
was unavailable, so the conditional live S3 portion skipped independently of
those seven ignored tests. Merged runtime and test files match the measured
candidate exactly. Primary workspace debug/release binaries are rebuilt for
current source; these are local source binaries, not a newly published package.

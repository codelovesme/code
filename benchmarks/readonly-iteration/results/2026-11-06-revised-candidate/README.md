# Revised read-only candidate — rejected

Frozen candidate `991d238e92da52a2fb83c57614499676424e8755` against `f18461d9706406a484ded1955c1d6ce62c3f4c54`.
[Hosted run 42284068886](https://github.com/codelovesme/code/actions/runs/42284068886).

The same preregistered 25-case/750-sample gate rejects this candidate: no large weighted/nearest task reaches 10% elapsed reduction. No material regression remains. Every result is retained; different hosts between trials are not combined.

| Work | Before ms | After ms | Change |
|---|---:|---:|---:|
| ascii_characters | 12.706 | 12.764 | +0.46% |
| binary_trees | 6.794 | 6.722 | -1.06% |
| config_4k | 0.953 | 0.882 | -7.49% |
| config_64k | 3.140 | 3.086 | -1.72% |
| dictionary | 3.953 | 3.977 | +0.62% |
| document_1m | 118.915 | 118.515 | -0.34% |
| document_64k | 6.369 | 6.242 | -1.99% |
| matrix_128 | 6.195 | 6.163 | -0.52% |
| matrix_64 | 1.529 | 1.497 | -2.09% |
| merge_16384 | 2.901 | 2.841 | -2.06% |
| merge_65536 | 10.060 | 9.918 | -1.41% |
| nearest_262144 | 48.402 | 45.514 | -5.97% |
| nearest_32768 | 6.025 | 5.794 | -3.84% |
| points | 1.478 | 1.445 | -2.23% |
| sieve_100000 | 2.557 | 2.525 | -1.22% |
| sieve_1000000 | 20.854 | 20.123 | -3.51% |
| snapshots | 191.748 | 194.062 | +1.21% |
| spectral_norm | 118.251 | 117.909 | -0.29% |
| string_build | 1.434 | 1.442 | +0.56% |
| sum_squares | 7.025 | 7.027 | +0.03% |
| threshold_262144 | 17.174 | 17.207 | +0.19% |
| threshold_32768 | 2.800 | 2.808 | +0.26% |
| unicode_characters | 29.062 | 29.512 | +1.55% |
| weighted_262144 | 57.475 | 56.710 | -1.33% |
| weighted_32768 | 7.206 | 7.170 | -0.50% |

Both known string controls produce byte-identical native programs before/after. Instruction-count savings alone did not establish an acceptable elapsed gain. No claim that a specific hardware bottleneck caused the gap is made without hardware/time profiles. Source patches remain archived, not applied to main.

[All raw samples](samples.csv) · [Complete report](report.json) · [Fixed gate audit](gate.json).

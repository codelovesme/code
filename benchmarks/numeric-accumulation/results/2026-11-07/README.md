# Numeric accumulation elapsed result — accepted

Candidate `8206a24c01a57bad6e1f17a7b4a3018d350a09e3` vs fixed source baseline
`3c6cbae0d969596948760ec5df14ff6a3e844f0f`, both Code 2.25.0. Fresh hosted
[run 42316511222](https://github.com/codelovesme/code/actions/runs/42316511222),
2026-11-07; same toolchain, independent build directories, one allowed CPU,
15 shuffled serial launches of each variant. All 900 samples / 60 summaries
and source/native hashes independently verified. No samples discarded.

**Accepted under the unchanged preregistered gate:** large threshold median
27.864 -> 19.692 ms (-29.33%); none of 30 cases meets the material-regression
definition. Small slowdowns remain visible below. Native compilation is
excluded; startup, input construction, queries, result checks and cleanup are
included. Host physical isolation/frequency is not guaranteed.

These are matched source results, not released Code/C/Rust rankings. Do not
compare absolute times with different-host 261/262/public-site datasets.
Snapshots includes 34,078,720 element additions while validating saved
versions; its reduction does not isolate snapshot creation or memory handling.

| Workload | Before ms | After ms | Elapsed change | Before MAD ms | After MAD ms |
|---|---:|---:|---:|---:|---:|
| ascii_characters | 22.785 | 22.797 | +0.05% | 0.058 | 0.094 |
| binary_trees | 9.870 | 9.911 | +0.41% | 0.057 | 0.028 |
| config_4k | 1.022 | 1.044 | +2.18% | 0.016 | 0.017 |
| config_64k | 4.258 | 4.404 | +3.43% | 0.015 | 0.031 |
| dictionary | 6.231 | 6.219 | -0.20% | 0.046 | 0.050 |
| document_1m | 187.569 | 187.367 | -0.11% | 0.451 | 0.435 |
| document_64k | 10.886 | 10.955 | +0.63% | 0.084 | 0.074 |
| list_alias | 0.928 | 0.935 | +0.74% | 0.021 | 0.018 |
| matrix_128 | 10.177 | 10.200 | +0.22% | 0.070 | 0.061 |
| matrix_64 | 2.105 | 2.127 | +1.04% | 0.022 | 0.019 |
| merge_16384 | 4.542 | 4.519 | -0.51% | 0.056 | 0.029 |
| merge_65536 | 17.326 | 17.299 | -0.16% | 0.093 | 0.071 |
| mixed_string | 10.534 | 10.543 | +0.08% | 0.074 | 0.129 |
| nearest_262144 | 66.329 | 66.343 | +0.02% | 0.338 | 0.544 |
| nearest_32768 | 8.280 | 8.298 | +0.21% | 0.087 | 0.119 |
| numeric_alias | 3.834 | 2.654 | -30.78% | 0.032 | 0.030 |
| numeric_take | 3.492 | 2.706 | -22.50% | 0.028 | 0.020 |
| object_alias | 6.291 | 6.335 | +0.68% | 0.034 | 0.018 |
| points | 1.959 | 1.940 | -0.98% | 0.029 | 0.010 |
| sieve_100000 | 3.437 | 3.406 | -0.88% | 0.032 | 0.052 |
| sieve_1000000 | 28.219 | 28.338 | +0.42% | 0.423 | 0.487 |
| snapshots | 335.113 | 218.638 | -34.76% | 0.315 | 0.173 |
| spectral_norm | 155.306 | 155.336 | +0.02% | 0.188 | 0.286 |
| string_build | 2.034 | 2.027 | -0.33% | 0.024 | 0.020 |
| sum_squares | 10.109 | 10.094 | -0.15% | 0.023 | 0.036 |
| threshold_262144 | 27.864 | 19.692 | -29.33% | 0.037 | 0.041 |
| threshold_32768 | 4.179 | 3.148 | -24.67% | 0.023 | 0.025 |
| unicode_characters | 62.937 | 62.906 | -0.05% | 0.104 | 0.089 |
| weighted_262144 | 82.397 | 82.451 | +0.07% | 0.424 | 0.720 |
| weighted_32768 | 9.828 | 9.882 | +0.55% | 0.094 | 0.057 |

Raw CSV and report JSON retain every sample, median/range/MAD/RSS count,
compiler/source/program/wrapper identity and environment. RSS is not the
acceptance target and is not a new memory-management claim.

Reproduce at the frozen candidate revision using its original workflow, now
preserved in `../../archived-workflow.yml`. Its exact source versions matter;
current main includes a separate owner 2.26 release whose runtime/codegen are
unchanged apart from this retained numeric shortcut. No release/site change is
made by this performance study.

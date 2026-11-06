# Additional compiled workloads — all outcomes

Fresh hosted run [42260715585](https://github.com/codelovesme/code/actions/runs/42260715585); public Code v2.24.1.
Frozen source `600e4f335bc9ed1c7451f8966198948a0d64a321`; sampling 2026-11-06T17:20:39+00:00–2026-11-06T17:20:48+00:00.

All 360 samples / 24 groups remain. Same VM, pinned CPU; 15 fresh process launches per variant. Compilation excluded, input construction/query/checks/cleanup/startup included. Values below are median milliseconds; lower is better.

| Work / items | Code | C O2 | C O3 | Rust O3 | Code / fastest competitor |
|---|---:|---:|---:|---:|---:|
| weighted_32768 | 9.438 | 2.522 | 2.520 | 2.888 | 3.74× |
| weighted_262144 | 82.002 | 15.940 | 15.866 | 17.176 | 5.17× |
| threshold_32768 | 4.044 | 1.146 | 1.162 | 1.799 | 3.53× |
| threshold_262144 | 27.069 | 4.759 | 4.793 | 8.409 | 5.69× |
| nearest_32768 | 7.864 | 1.242 | 1.264 | 1.596 | 6.33× |
| nearest_262144 | 64.983 | 5.748 | 5.847 | 6.921 | 11.31× |

Preregistered competitive wins: **none**.

[Protocol and implementation scope](../../README.md) · [Complete JSON](report.json) · [All raw samples](samples.csv).

Every implementation checks an independent exact-integer oracle. C uses native typed records/arrays and integer indexing; Rust uses typed Vec and range iteration; Code uses ordinary generic values/objects and numeric indexing. All use identical data, 16 queries and arithmetic order; no deliberate copies or barriers. The deterministic periodic inputs are a limited workload, not a claim about every dataset or algorithm. Both C O2 and O3 are retained. No compiler/memory advantage is substituted for runtime results.

A competitive win requires >=10% lead over C O2, C O3 and Rust O3 and a lead larger than twice the sum of MADs. Small/startup-sensitive differences need repetition. No old/new-host ranks or percentages are combined. No website change follows from an uncompetitive pilot.

## Every range and dispersion

| Work | Mode | Median ms | Min–max ms | MAD ms |
|---|---|---:|---:|---:|
| nearest_262144 | C O2 | 5.748 | 5.513–6.150 | 0.153 |
| nearest_262144 | C O3 | 5.847 | 5.533–6.365 | 0.224 |
| nearest_262144 | Code release | 64.983 | 63.768–67.858 | 0.523 |
| nearest_262144 | Rust O3 | 6.921 | 6.634–7.386 | 0.161 |
| nearest_32768 | C O2 | 1.242 | 1.214–1.291 | 0.016 |
| nearest_32768 | C O3 | 1.264 | 1.236–1.413 | 0.024 |
| nearest_32768 | Code release | 7.864 | 7.527–8.357 | 0.244 |
| nearest_32768 | Rust O3 | 1.596 | 1.549–1.713 | 0.016 |
| threshold_262144 | C O2 | 4.759 | 4.724–5.021 | 0.028 |
| threshold_262144 | C O3 | 4.793 | 4.701–4.982 | 0.031 |
| threshold_262144 | Code release | 27.069 | 26.846–27.392 | 0.176 |
| threshold_262144 | Rust O3 | 8.409 | 8.337–9.376 | 0.038 |
| threshold_32768 | C O2 | 1.146 | 1.116–1.212 | 0.018 |
| threshold_32768 | C O3 | 1.162 | 1.135–1.524 | 0.022 |
| threshold_32768 | Code release | 4.044 | 3.996–4.376 | 0.029 |
| threshold_32768 | Rust O3 | 1.799 | 1.764–1.956 | 0.019 |
| weighted_262144 | C O2 | 15.940 | 15.625–16.441 | 0.094 |
| weighted_262144 | C O3 | 15.866 | 15.703–16.430 | 0.130 |
| weighted_262144 | Code release | 82.002 | 81.270–99.467 | 0.698 |
| weighted_262144 | Rust O3 | 17.176 | 16.932–17.478 | 0.134 |
| weighted_32768 | C O2 | 2.522 | 2.488–2.651 | 0.015 |
| weighted_32768 | C O3 | 2.520 | 2.471–2.611 | 0.021 |
| weighted_32768 | Code release | 9.438 | 9.132–10.628 | 0.204 |
| weighted_32768 | Rust O3 | 2.888 | 2.828–3.324 | 0.025 |

## Follow-up supported by the profile

No workload wins against both C/Rust here; none is promoted as a competitive
website row. The separate [local weighted profile](../../profiles/weighted-32768/metadata.json)
records 74,834,511 dynamic instructions, of which 20,447,223 (27.32%) are self
instructions in `code_copy`. Object construction, retain/release and loop work
are also present. This is an instruction profile of the complete program, not
a CPU-time share or proof that the query loop alone causes those copies.

Next investigate separating initialization from the read-only query phase and
whether repeated element/field reads can borrow or use proven scalar values
without temporary ownership churn. Preserve independent values, lifetime and
fallback checks. This is an optimization hypothesis, not an implemented fix or
a promise to close the 5–11x large-case gap. A successful candidate must retain
complete same-host before/after evidence and broader regression controls.

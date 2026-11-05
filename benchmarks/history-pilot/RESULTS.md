# Retained-history pilot — public Code v2.23.0

Measured 5 November 2026, 20:01:25–20:01:42 UTC in a fresh Ubuntu hosted job:
[run 42099367294](https://github.com/codelovesme/code/actions/runs/42099367294).
Harness/source commit: `28d14b14a4a26a37ff7856dcde60f9b8c25349b3`.

[All samples](results/2026-11-05-v2.23.0/samples.csv) · [Full identities/report](results/2026-11-05-v2.23.0/report.json) · [Protocol/representations](README.md).

## Findings

- Large unchanged configuration payloads are a useful strength: automatic Code value sharing avoids eager deep copying. For 64 sections × 64 KiB and 65 retained versions, Code takes **3.437 ms / 1.734 MiB**, versus Rust eager clones **155.901 ms / 262.164 MiB**: **45.35× faster and 99.34% lower peak RSS** on this complete program.
- This does not establish superiority over optimized C/Rust. Explicit C sharing takes **0.599 ms / 1.473 MiB**; explicit Rust Rc sharing **0.842 ms / 2.223 MiB**. Code is **4.08× slower than Rust shared** here. Small process-RSS differences between shared variants can reflect startup/accounting, not lower heap use.
- The smaller configuration case also favors Code versus eager clones (11.60× Rust eager time / Code time), while explicit-sharing C/Rust remain faster. Both sizes were fixed before timing.
- Document revision is a counterexample: Code takes **444.049 ms / 70.469 MiB** for 1 MiB documents, versus Rust shared **36.001 ms / 68.234 MiB**. Code is **12.33× slower** and uses slightly more process RSS. At 64 KiB Code is also slower. These results are retained, not hidden.
- Scope is retained immutable data and complete-program validation/cleanup. Code string edits/validation use slices and concatenation; C/Rust use mutable private buffers. Pointer shortcuts and representation differences matter. This is not an isolated COW-operation test or a universal language ranking.

## All results

Each entry: 15 fresh launches, medians and full ranges. Ratios use unrounded medians. Building is excluded; startup, input construction, 65 retained versions, edits, full-value verification and cleanup are included.

| Case | Implementation | Median ms | Range ms | Peak MiB | Peak range MiB |
|---|---|---:|---:|---:|---:|
| config_4k | C eager | 9.958 | 9.629–10.488 | 17.973 | 17.973–17.973 |
| config_4k | C shared | 0.572 | 0.533–0.635 | 1.473 | 1.469–1.473 |
| config_4k | Code automatic | 0.838 | 0.817–0.944 | 1.609 | 1.605–1.609 |
| config_4k | Rust eager | 9.719 | 9.100–10.287 | 18.355 | 18.297–18.359 |
| config_4k | Rust shared | 0.773 | 0.705–0.822 | 2.078 | 2.066–2.219 |
| config_64k | C eager | 155.934 | 151.856–158.144 | 261.723 | 261.660–261.723 |
| config_64k | C shared | 0.599 | 0.553–0.675 | 1.473 | 1.469–1.473 |
| config_64k | Code automatic | 3.437 | 3.365–3.567 | 1.734 | 1.730–1.734 |
| config_64k | Rust eager | 155.901 | 152.622–160.751 | 262.164 | 262.121–262.289 |
| config_64k | Rust shared | 0.842 | 0.804–0.920 | 2.223 | 2.191–2.348 |
| document_1m | C eager | 36.305 | 34.628–36.938 | 68.473 | 68.410–68.473 |
| document_1m | C shared | 35.861 | 34.507–39.483 | 67.473 | 67.410–67.473 |
| document_1m | Code automatic | 444.049 | 441.522–454.111 | 70.469 | 70.465–70.473 |
| document_1m | Rust eager | 36.071 | 34.621–37.069 | 69.227 | 69.184–69.289 |
| document_1m | Rust shared | 36.001 | 35.290–36.601 | 68.234 | 68.172–68.234 |
| document_64k | C eager | 2.900 | 2.703–4.370 | 5.473 | 5.469–5.473 |
| document_64k | C shared | 3.053 | 2.862–3.138 | 5.348 | 5.344–5.348 |
| document_64k | Code automatic | 26.583 | 26.449–26.808 | 5.734 | 5.727–5.734 |
| document_64k | Rust eager | 3.163 | 2.935–3.322 | 6.105 | 6.051–6.121 |
| document_64k | Rust shared | 3.143 | 2.926–3.561 | 6.109 | 6.008–6.168 |

## Verification and interpretation

All 300 CSV rows match JSON field by field; all 20 group medians/minima/maxima/MAD/zero-RSS counts were independently recomputed from CSV. All 20 downloaded native-artifact and generated-source hashes match the report. Generated inputs match the locally validated final sources. No samples were dropped; no RSS sample is zero.

The actual public v2.23.0 launcher/driver hashes were enforced. GCC 13.3.0 O2 and Rust 1.99.1 O3 use native CPU targeting; Code uses its public release pipeline. Exact versions, binaries, runner image, lscpu, affinity, pressure, timestamps and source hashes are in the report. Serial seeded sampling used one allowed logical CPU. Physical-host isolation/fixed frequency is not promised.

All 20 variants first passed untimed local and hosted value checks. Eight C variants passed local AddressSanitizer/UndefinedBehaviorSanitizer/leak checking; four Code variants passed native leak checking. Generated Code is formatter-clean. Full workspace: 340 passed, zero failed, seven ignored, zero filtered (69 summaries).

Preparation history: a first Code draft used unsupported `*=` syntax; a second tried unsupported string-part assignment. These were corrected to ordinary assignment and the published slice/concatenation API. Every fixed pre-registered case was then validated and measured. No runtime feature was added or result excluded.

## Existing website evidence

The earlier host already measured lower Code process peak RSS than both C/Rust for dictionary (1.480 vs 1.660/2.094 MiB), points (1.609 vs 1.723/1.996 MiB) and spectral norm (1.484 vs 2.000/2.090 MiB). Those are small-footprint observations, not broad heap-management superiority. Their timing values/host are separate from this pilot.

The frozen November 4 report and live website are unchanged. Do not paste these new-host numbers into its old rankings. A future comparison could explain automatic versus explicit sharing with all five implementations and retain the document counterexample; publishing only the eager-clone win would omit an essential limitation.

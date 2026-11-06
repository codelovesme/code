# Public Code v2.23.2 — complete current website evidence

Measured 6 November 2026 on one fresh Ubuntu 24.04 VM:
[run 42139731690](https://github.com/codelovesme/code/actions/runs/42139731690).
Comparison samples: 00:50:48–00:51:37 UTC; sharing samples: 00:51:40–00:52:00 UTC.
Both samplers used CPU 0, the same public launcher/driver and the same toolchain.
Measurement source `80d3b2c888577818976cac53b0631dbb07270e56`; public release
source `1e57f007eb002074640b6b33f6661dab84581923`.

[Comparison JSON](comparison/report.json) · [Comparison CSV](comparison/samples.csv) · [Sharing JSON](history/report.json) · [Sharing CSV](history/samples.csv).

All **785 samples / 55 entries** are retained: the original 35 comparison entries
(485 samples) plus all 20 sharing cases (300 samples). No new compiled-algorithm
cases or chart categories were introduced. All unfavorable results remain.
This fresh-host dataset replaces the current charts as a whole; older reports
remain unchanged and are not mixed into these rankings.

## Findings and scope

On the ten-million-iteration numeric loop, native Code takes 10.073 ms,
Rust O3 10.277 ms, C O2/O3 12.307/12.318 ms. This is a complete-program result
including startup/checks, not general language superiority. The five broader
workloads retain their own results and different typed/generic layouts.

Configuration histories demonstrate automatic sharing versus eager copies;
explicit sharing is faster. Text-document revisions remain a counterexample:
Code is slower. Matrix snapshots save memory versus eager copies while taking
more runtime; explicit C/Rust COW is faster and leaner. Small process-RSS
floor differences are not proof of heap-management superiority.

## Every measured outcome

Median and minimum–maximum. Runtime groups have 15 fresh launches, compile groups
five. Code cold includes compiler work; compiler memory belongs to compilation,
not application footprint. Startup, construction, computation, verification and
cleanup are included for prebuilt programs. RSS is largest process resident peak,
not live heap or a summed process tree. OS caches remain warm. Source-launch
strategies differ (native cache, forced interpreter, JIT and bytecode runtime).

### Original comparison

| Program / group | Entry | Median ms (range) | Peak MiB (range) |
|---|---|---:|---:|
| binary_trees / prebuilt | C O2 | 3.663 (3.598–3.715) | 1.852 (1.848–1.914) |
| binary_trees / prebuilt | Code release | 9.807 (9.667–9.998) | 2.859 (2.855–2.859) |
| binary_trees / prebuilt | Rust O3 | 3.912 (3.878–4.013) | 2.230 (2.223–2.285) |
| binary_trees / source | Code interpret | 105.985 (104.585–114.977) | 31.977 (31.414–32.371) |
| dictionary / prebuilt | C O2 | 4.016 (3.814–4.188) | 1.660 (1.660–1.660) |
| dictionary / prebuilt | Code release | 6.183 (6.093–10.240) | 1.480 (1.480–1.480) |
| dictionary / prebuilt | Rust O3 | 2.878 (2.839–2.943) | 2.082 (2.004–2.125) |
| dictionary / source | Code interpret | 22.681 (22.402–23.504) | 30.008 (29.789–30.383) |
| points / prebuilt | C O2 | 0.850 (0.811–0.885) | 1.723 (1.723–1.723) |
| points / prebuilt | Code release | 1.918 (1.879–3.776) | 1.609 (1.605–1.609) |
| points / prebuilt | Rust O3 | 1.179 (1.152–1.217) | 1.996 (1.965–2.121) |
| snapshots / prebuilt | C eager clone | 185.586 (183.523–188.466) | 261.598 (261.535–261.598) |
| snapshots / prebuilt | C explicit COW | 36.214 (36.082–36.432) | 7.223 (7.219–7.223) |
| snapshots / prebuilt | Code automatic COW | 335.291 (334.320–340.950) | 13.984 (13.980–13.984) |
| snapshots / prebuilt | Rust Rc COW | 36.879 (36.635–40.141) | 7.992 (7.965–8.121) |
| snapshots / prebuilt | Rust Vec clone | 186.006 (183.656–189.634) | 262.059 (262.000–262.184) |
| spectral_norm / prebuilt | C O2 | 69.666 (69.435–70.734) | 2.074 (1.961–2.086) |
| spectral_norm / prebuilt | Code release | 155.337 (154.789–163.286) | 1.484 (1.484–1.484) |
| spectral_norm / prebuilt | Rust O3 | 73.967 (73.772–74.505) | 1.992 (1.973–2.102) |
| string_build / prebuilt | C O2 | 1.862 (1.793–1.932) | 1.602 (1.598–1.664) |
| string_build / prebuilt | Code release | 1.904 (1.875–3.369) | 1.605 (1.602–1.605) |
| string_build / prebuilt | Rust O3 | 1.408 (1.364–1.467) | 1.980 (1.949–2.031) |
| sum_squares / compile | C O2 | 36.074 (34.923–36.485) | 23.332 (23.188–23.422) |
| sum_squares / compile | C O3 | 35.687 (35.148–36.392) | 23.895 (23.766–23.906) |
| sum_squares / compile | Code release | 37.545 (36.664–37.768) | 40.453 (40.289–40.508) |
| sum_squares / compile | Rust O3 | 77.085 (74.532–78.188) | 99.609 (98.398–99.738) |
| sum_squares / prebuilt | C O2 | 12.307 (12.269–12.349) | 1.164 (1.164–1.164) |
| sum_squares / prebuilt | C O3 | 12.318 (12.233–12.446) | 1.164 (1.164–1.164) |
| sum_squares / prebuilt | Code release | 10.073 (10.006–10.192) | 1.355 (1.352–1.355) |
| sum_squares / prebuilt | Rust O3 | 10.277 (10.207–10.350) | 1.977 (1.938–2.055) |
| sum_squares / source | Code cold run | 47.661 (47.133–48.618) | 40.070 (39.848–40.234) |
| sum_squares / source | Code interpret | 309.746 (298.278–333.363) | 30.008 (29.438–30.172) |
| sum_squares / source | Code warm run | 10.937 (10.860–11.039) | 1.945 (1.930–2.043) |
| sum_squares / source | Node/V8 | 39.526 (38.224–41.433) | 49.898 (49.625–49.934) |
| sum_squares / source | Python | 1099.656 (1046.594–1266.596) | 10.270 (10.223–10.285) |

### Configuration and document histories

| Program / group | Entry | Median ms (range) | Peak MiB (range) |
|---|---|---:|---:|
| config_4k / prebuilt | C eager | 11.152 (10.803–11.449) | 17.848 (17.844–17.848) |
| config_4k / prebuilt | C shared | 0.706 (0.622–0.730) | 1.348 (1.285–1.348) |
| config_4k / prebuilt | Code automatic | 1.046 (0.984–1.121) | 1.484 (1.480–1.484) |
| config_4k / prebuilt | Rust eager | 11.105 (10.952–11.450) | 18.191 (18.164–18.289) |
| config_4k / prebuilt | Rust shared | 0.928 (0.888–1.017) | 1.977 (1.973–2.102) |
| config_64k / prebuilt | C eager | 168.650 (164.737–186.373) | 261.598 (261.594–261.598) |
| config_64k / prebuilt | C shared | 0.717 (0.654–0.817) | 1.348 (1.285–1.348) |
| config_64k / prebuilt | Code automatic | 4.418 (4.352–4.590) | 1.609 (1.609–1.609) |
| config_64k / prebuilt | Rust eager | 168.294 (167.018–170.221) | 262.070 (262.055–262.164) |
| config_64k / prebuilt | Rust shared | 0.996 (0.977–1.101) | 2.109 (2.086–2.164) |
| document_1m / prebuilt | C eager | 41.230 (39.638–42.219) | 68.348 (68.285–68.348) |
| document_1m / prebuilt | C shared | 41.271 (40.450–42.341) | 67.348 (67.285–67.348) |
| document_1m / prebuilt | Code automatic | 510.264 (508.715–512.329) | 70.344 (70.340–70.348) |
| document_1m / prebuilt | Rust eager | 41.470 (40.193–42.936) | 69.105 (69.051–69.164) |
| document_1m / prebuilt | Rust shared | 41.306 (40.684–41.784) | 68.090 (68.047–68.102) |
| document_64k / prebuilt | C eager | 3.387 (3.180–3.448) | 5.348 (5.344–5.348) |
| document_64k / prebuilt | C shared | 3.385 (3.150–3.452) | 5.223 (5.160–5.223) |
| document_64k / prebuilt | Code automatic | 30.758 (30.621–31.220) | 5.609 (5.605–5.609) |
| document_64k / prebuilt | Rust eager | 3.556 (3.397–3.808) | 5.945 (5.930–5.977) |
| document_64k / prebuilt | Rust shared | 3.568 (3.423–3.758) | 5.957 (5.918–5.988) |

## Verification and reproducibility

Independent `benchmarks/website-evidence/verify_current.py` matched all CSV/JSON
fields and sample indices; recomputed every median/min/max/MAD/zero-RSS count;
verified native/source/wrapper/warm-cache artifact hashes and recorded source
bytes; and confirmed shared job/CPU/OS, release source and launcher/driver hashes.
Both generated sharing inputs match the untimed locally validated sources.
No samples were excluded. See JSON for zero-RSS counts and full identities.

C/Rust use GCC 13.3.0 O2/O3 and rustc 1.99.1 O3/native CPU targeting; Code uses
the actual public v2.23.2 pipeline. Node 22/Python versions, V8 identity,
runner image, binaries, lscpu/affinity/pressure and all timing boundaries are
recorded in the reports. Physical-host isolation/fixed CPU frequency is not
guaranteed. Compilation finishes before runtime sampling; unrelated build/
profile/test processes are rejected. This is not an allocation-count or kernel-
only study. Typed versus generic representations and validation costs differ.

The reviewed release manifest fixes the tarball digest and actual launcher/
driver hashes. Development binaries with a matching version are rejected;
missing companion drivers also produce an explicit error. The old v2.23.0
sampler default remains supported; old CSV/JSON and workload sources are frozen.

Local smoke validated all existing programs and cache preparation without
collecting timings. A negative identity probe initially used a development
binary without a driver; explicit paired-bundle diagnostics were added and the
guards rerun. The independent verifier initially expected a flat wrapper object;
it was adapted to the comparison report's existing nested artifact schema.
Neither issue excluded a sample or changed any measured program or outcome.

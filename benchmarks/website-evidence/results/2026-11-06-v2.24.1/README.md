# Public Code v2.24.1 — complete current website evidence

Measured on one fresh Ubuntu VM: [run 42212047785](https://github.com/codelovesme/code/actions/runs/42212047785).
Comparison: 2026-11-06T11:41:30+00:00–2026-11-06T11:42:14+00:00.
Histories: 2026-11-06T11:42:17+00:00–2026-11-06T11:42:31+00:00.
Both use CPU 0, the same public launcher/driver and toolchain.
Measurement source `09bd1a072aca9af0b2870a98c112b01434c618c7`; public release source `97d3df30eda7943100ed128880bbc3c2457ad055`.

[Comparison JSON](comparison/report.json) · [Comparison CSV](comparison/samples.csv) ·
[Histories JSON](history/report.json) · [Histories CSV](history/samples.csv).

All **785 samples / 55 entries** remain: 35 existing comparison entries (485
samples) and all 20 retained-history entries (300 samples). No new chart or
compiled-algorithm table was added. Every unfavorable result stays. This
one-host dataset replaces current charts as a whole; earlier host/source
results remain historical and are not mixed into current ranks.

Startup, construction, algorithm, complete result verification and cleanup are
included for prebuilt programs; compilation is excluded. Cold source launches
include compilation; warm source runs reuse native artifacts. Compiler RSS
belongs to compilation, not application memory. Runtime entries use 15 fresh
launches, compiler entries five. OS caches remain warm. Memory is largest
process resident peak, not live heap, allocations or summed process-tree memory.

Generic Code values, typed C/Rust data, eager copies and explicit sharing differ.
Configuration histories check sharing-friendly edits; document histories copy
changed text and build expected values differently. These are complete-program
results, not isolated COW/update or arithmetic-kernel timings. No language-wide
performance or memory-management superiority is implied.

## Every measured outcome

### Original comparison

| Program / group | Entry | Median ms (range) | Peak MiB (range) |
|---|---|---:|---:|
| binary_trees / prebuilt | C O2 | 3.242 (2.996–3.681) | 1.977 (1.973–1.977) |
| binary_trees / prebuilt | Code release | 8.299 (7.647–9.287) | 2.922 (2.918–2.922) |
| binary_trees / prebuilt | Rust O3 | 3.466 (3.214–4.012) | 2.383 (2.328–2.473) |
| binary_trees / source | Code interpret | 91.864 (85.214–98.600) | 32.000 (31.676–32.500) |
| dictionary / prebuilt | C O2 | 3.118 (2.852–3.541) | 1.785 (1.785–1.848) |
| dictionary / prebuilt | Code release | 5.173 (4.700–5.600) | 1.547 (1.543–1.547) |
| dictionary / prebuilt | Rust O3 | 2.442 (2.195–2.671) | 2.195 (2.125–2.250) |
| dictionary / source | Code interpret | 19.638 (18.321–21.784) | 30.340 (29.766–30.672) |
| points / prebuilt | C O2 | 0.810 (0.710–0.971) | 1.785 (1.785–1.785) |
| points / prebuilt | Code release | 1.866 (1.689–2.099) | 1.734 (1.672–1.734) |
| points / prebuilt | Rust O3 | 1.320 (1.238–1.544) | 2.191 (2.121–2.246) |
| snapshots / prebuilt | C eager clone | 182.916 (169.917–200.787) | 261.727 (261.660–261.727) |
| snapshots / prebuilt | C explicit COW | 34.866 (31.578–38.995) | 7.352 (7.348–7.352) |
| snapshots / prebuilt | Code automatic COW | 311.290 (284.549–342.468) | 14.109 (14.105–14.109) |
| snapshots / prebuilt | Rust Rc COW | 34.464 (31.662–38.311) | 8.121 (8.062–8.219) |
| snapshots / prebuilt | Rust Vec clone | 183.327 (169.807–192.716) | 262.133 (262.113–262.309) |
| spectral_norm / prebuilt | C O2 | 60.937 (57.659–68.591) | 2.199 (2.090–2.219) |
| spectral_norm / prebuilt | Code release | 142.244 (133.440–154.279) | 1.613 (1.609–1.613) |
| spectral_norm / prebuilt | Rust O3 | 71.357 (65.555–77.668) | 2.121 (2.078–2.230) |
| string_build / prebuilt | C O2 | 1.457 (1.295–1.637) | 1.727 (1.723–1.789) |
| string_build / prebuilt | Code release | 1.816 (1.682–2.069) | 1.730 (1.727–1.730) |
| string_build / prebuilt | Rust O3 | 1.270 (1.141–1.455) | 2.105 (2.105–2.223) |
| sum_squares / compile | C O2 | 38.023 (36.768–169.341) | 24.352 (24.316–24.465) |
| sum_squares / compile | C O3 | 38.094 (37.003–232.487) | 24.840 (24.789–24.957) |
| sum_squares / compile | Code release | 36.895 (36.349–37.821) | 40.559 (40.328–40.719) |
| sum_squares / compile | Rust O3 | 79.561 (75.603–81.333) | 98.840 (98.699–100.023) |
| sum_squares / prebuilt | C O2 | 11.472 (10.091–12.544) | 1.227 (1.223–1.289) |
| sum_squares / prebuilt | C O3 | 11.051 (10.084–12.240) | 1.227 (1.223–1.227) |
| sum_squares / prebuilt | Code release | 9.096 (8.738–10.915) | 1.355 (1.352–1.355) |
| sum_squares / prebuilt | Rust O3 | 10.091 (8.947–10.764) | 2.070 (2.043–2.133) |
| sum_squares / source | Code cold run | 46.201 (41.827–49.497) | 40.266 (39.867–40.402) |
| sum_squares / source | Code interpret | 262.625 (227.168–285.445) | 29.941 (29.441–30.328) |
| sum_squares / source | Code warm run | 10.145 (9.468–20.704) | 2.152 (2.055–2.172) |
| sum_squares / source | Node/V8 | 36.340 (34.359–40.686) | 50.059 (50.008–50.059) |
| sum_squares / source | Python | 1048.370 (931.208–1145.757) | 10.227 (10.211–10.262) |

### Configuration and document histories

| Program / group | Entry | Median ms (range) | Peak MiB (range) |
|---|---|---:|---:|
| config_4k / prebuilt | C eager | 11.304 (10.115–12.194) | 17.973 (17.910–17.973) |
| config_4k / prebuilt | C shared | 0.642 (0.560–0.747) | 1.473 (1.410–1.473) |
| config_4k / prebuilt | Code automatic | 0.954 (0.815–1.099) | 1.609 (1.605–1.609) |
| config_4k / prebuilt | Rust eager | 11.518 (10.118–12.249) | 18.355 (18.301–18.355) |
| config_4k / prebuilt | Rust shared | 0.885 (0.815–1.186) | 2.102 (2.094–2.211) |
| config_64k / prebuilt | C eager | 179.093 (162.401–186.183) | 261.723 (261.719–261.723) |
| config_64k / prebuilt | C shared | 0.664 (0.593–0.782) | 1.473 (1.465–1.473) |
| config_64k / prebuilt | Code automatic | 3.855 (3.504–4.042) | 1.734 (1.730–1.734) |
| config_64k / prebuilt | Rust eager | 179.606 (167.112–188.293) | 262.230 (262.168–262.246) |
| config_64k / prebuilt | Rust shared | 0.957 (0.808–1.084) | 2.227 (2.180–2.340) |
| document_1m / prebuilt | C eager | 41.304 (38.027–43.341) | 68.473 (68.410–68.473) |
| document_1m / prebuilt | C shared | 41.345 (37.992–42.658) | 67.473 (67.469–67.473) |
| document_1m / prebuilt | Code automatic | 177.895 (165.850–186.633) | 70.473 (70.465–70.473) |
| document_1m / prebuilt | Rust eager | 41.373 (37.755–43.549) | 69.164 (69.137–69.289) |
| document_1m / prebuilt | Rust shared | 41.824 (37.819–43.276) | 68.234 (68.180–68.234) |
| document_64k / prebuilt | C eager | 3.342 (2.950–3.618) | 5.473 (5.469–5.473) |
| document_64k / prebuilt | C shared | 3.323 (2.996–3.864) | 5.348 (5.285–5.348) |
| document_64k / prebuilt | Code automatic | 9.978 (8.947–10.293) | 5.734 (5.730–5.734) |
| document_64k / prebuilt | Rust eager | 3.710 (3.165–3.926) | 6.031 (5.973–6.164) |
| document_64k / prebuilt | Rust shared | 3.662 (3.320–4.029) | 6.078 (6.074–6.168) |

## Verification and limits

Independent verify_current.py matches every CSV/JSON field and sample index,
all medians/min/max/MAD/zero-RSS counts, native/source/wrapper/warm-cache bytes,
and matching job/CPU/OS/public source/binary identities. Both generated history
inputs match the locally smoke-validated sources. No samples are excluded.

Exact toolchain, runtime, V8, OS/CPU/runner image, affinity, pressure and binary
identities are in the reports. Physical isolation and fixed frequency are not
guaranteed. Preparation completes before runtime sampling; concurrent unrelated
compiler/test/profiler processes are rejected. Source-launch strategies include
cached native execution, forced bytecode interpreter, JIT and bytecode runtime.

The reviewed release manifest fixes tarball and launcher/driver SHA-256 values;
a same-version development binary is rejected. Keep the public pair together.
Historical samplers/data remain frozen. This publication does not silently
upgrade application module pins or deploy an application.

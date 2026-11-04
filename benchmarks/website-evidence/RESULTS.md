# Website claim evidence — public Code v2.23.0

Measured **4 November 2026, 22:23:35–22:24:24 UTC**, in
[GitHub run 41924830382](https://github.com/codelovesme/code/actions/runs/41924830382),
from harness/workload commit `1dd7741880c4233ba1473c1c392c2725ff67df17`.
This is the existing public release, not a new language build or release.

[Full report and fingerprints](results/2026-11-04-v2.23.0/report.json) ·
[All raw samples](results/2026-11-04-v2.23.0/samples.csv) ·
[Protocol, representations and historical audit](README.md).

## Main findings

- The native numeric loop is competitive with the typed C/Rust variants on
  this host: Code 10.082 ms, Rust 10.344 ms, GCC O2 12.302 ms. This supports a
  narrowly scoped numeric-kernel example, not “Code always runs at C/Rust speed.”
- On the five broader programs, Code takes **1.02–2.68× GCC O2's elapsed time**.
  It is slower than Rust in all five. Preserve those rows beside the best case.
- Numeric warm `code run` takes 10.963 ms, cold-cache run 49.012 ms, explicit
  interpretation 341.309 ms. These are different preparation paths. The cold
  run includes compilation; valid warm launches preserve the native artifact.
- The retained-matrix case demonstrates automatic structural sharing, but does
  not establish a general memory-management advantage over Rust. Code peaks at 13.984 MiB versus Rust Vec clones
  at 262.070 MiB (94.7% lower RSS); **Code is 1.81× slower** on that full program.
  Equivalent Rust Rc COW peaks at 8.004 MiB and takes 36.760 ms versus Code's
  334.822 ms: Code uses **1.75× RSS and 9.11× elapsed time**. Typed C COW is also
  faster and leaner. The full program includes initialization and full checksum
  validation, so these are not isolated copy-operation timings.

## Machine, identity and integrity

Fresh Ubuntu 24.04.5 hosted VM, Linux 7.0.0-1017-azure, glibc 2.39,
AMD EPYC 7763 model, **4 virtual CPUs**, pinned to logical CPU 0. Runner image
20261101.360.1, Microsoft hypervisor; CPU governor unavailable. This is not
bare-metal isolation or a claim of fixed frequency. Initial load average was
0.45/0.10/0.03; CPU pressure before/after is retained in JSON.

Code v2.23.0's official release source tag points to
`e7436b70ba337f706a9761a9380dd49118818c71`. Downloaded launcher/driver hashes
match the public bundle also used to verify the website examples. Other tools:
GCC/cc 13.3.0, GNU ld 2.42, Rust 1.99.1 (LLVM 23.1.1), Python 3.12.3,
Node 22.23.4 / V8 12.4.254.21-node.57. Code uses its public `--release` pipeline;
C uses O2 (O3 additionally for the numeric loop), Rust O3; C/Rust enable their
native CPU target. No Cargo build startup is attributed to rustc.

All 485 CSV rows were compared field-by-field to JSON; group statistics were
independently recomputed from CSV. The 24 prepared runtime artifact hashes,
workload/harness hashes, public CLI/driver hashes and 25 historical JSON/CSV
fingerprints were checked. Every target's assertions passed. No samples were
dropped. Runtime groups have 15 launches each and compiler groups five, in
serial shuffled order. Cold samples produced actual native executables; warm
executable inode/hash and all prepared runtime binaries stayed unchanged.

Local untimed preparation also passes all variants. Both new C representations
pass AddressSanitizer/UndefinedBehaviorSanitizer; the Code matrix passes its
native runtime leak check. Required workspace validation: 339 passed, zero
failed, seven ignored. These local checks are not the timing dataset.

## Prebuilt runtime and application peak RSS

Builds happen before these launches. Process startup, workload, checks, output
where present and cleanup are included. Generic/typed representations and
lookup algorithms differ as audited in README. Short rows can be dominated by
startup; few-hundred-KiB differences are process-footprint observations, not
proof of superior heap management.

| Workload | Mode | Median ms | Full range ms | Median peak MiB | Full peak range MiB | n |
|---|---|---:|---:|---:|---:|---:|
| binary_trees | C O2 | 3.652 | 3.607–4.081 | 1.852 | 1.848–1.914 | 15 |
| binary_trees | Code release | 9.786 | 9.715–10.221 | 2.859 | 2.855–2.859 | 15 |
| binary_trees | Rust O3 | 3.883 | 3.831–4.116 | 2.234 | 2.133–2.359 | 15 |
| dictionary | C O2 | 3.964 | 3.844–4.156 | 1.660 | 1.660–1.660 | 15 |
| dictionary | Code release | 6.391 | 6.287–6.603 | 1.480 | 1.477–1.480 | 15 |
| dictionary | Rust O3 | 2.886 | 2.842–3.093 | 2.094 | 2.055–2.117 | 15 |
| points | C O2 | 0.863 | 0.824–0.892 | 1.723 | 1.719–1.723 | 15 |
| points | Code release | 1.926 | 1.877–2.091 | 1.609 | 1.605–1.609 | 15 |
| points | Rust O3 | 1.170 | 1.129–1.236 | 1.996 | 1.965–2.121 | 15 |
| spectral_norm | C O2 | 69.128 | 69.001–71.266 | 2.000 | 1.953–2.082 | 15 |
| spectral_norm | Code release | 155.428 | 155.044–158.773 | 1.484 | 1.484–1.484 | 15 |
| spectral_norm | Rust O3 | 73.286 | 73.143–74.736 | 2.090 | 2.023–2.117 | 15 |
| string_build | C O2 | 1.859 | 1.789–1.917 | 1.602 | 1.598–1.664 | 15 |
| string_build | Code release | 1.895 | 1.861–2.028 | 1.605 | 1.602–1.605 | 15 |
| string_build | Rust O3 | 1.384 | 1.348–1.447 | 1.980 | 1.969–2.105 | 15 |
| sum_squares | C O2 | 12.302 | 12.250–12.431 | 1.164 | 1.160–1.164 | 15 |
| sum_squares | C O3 | 12.313 | 12.261–12.345 | 1.164 | 1.164–1.164 | 15 |
| sum_squares | Code release | 10.082 | 10.000–10.124 | 1.355 | 1.355–1.355 | 15 |
| sum_squares | Rust O3 | 10.344 | 10.301–10.391 | 1.980 | 1.949–2.012 | 15 |

## Retained snapshots: all representations

Each variant initializes 128×4096 float64 cells, retains 64 pre-update versions,
changes one diagonal cell per step, and validates all 65 full matrix sums.
C/Rust use dense 8-byte float64 cells; Code uses generic container values with
its own headers, capacity and counting costs. All five produce no output.
Eager whole-matrix copies and equivalent COW are distinct first-class baselines.

| Workload | Mode | Median ms | Full range ms | Median peak MiB | Full peak range MiB | n |
|---|---|---:|---:|---:|---:|---:|
| snapshots | C eager clone | 184.796 | 183.109–188.410 | 261.598 | 261.535–261.598 | 15 |
| snapshots | C explicit COW | 36.193 | 36.002–37.435 | 7.223 | 7.160–7.223 | 15 |
| snapshots | Code automatic COW | 334.822 | 334.243–337.989 | 13.984 | 13.984–13.984 | 15 |
| snapshots | Rust Rc COW | 36.760 | 36.489–36.895 | 8.004 | 7.977–8.125 | 15 |
| snapshots | Rust Vec clone | 184.954 | 183.978–208.322 | 262.070 | 262.000–262.094 | 15 |

## Source-run paths

These fresh launches accept source files. They include source preparation
where required; warm Code reuses an untimed native build. Node/V8 is a Node
process, not a browser measurement. The Code interpreter is 8.63× Node's time
and about 30% of CPython’s time on this numeric source program. Do not
present native warm Code versus interpreted competitors as one engine strategy.
This dataset says nothing about the site's separately pinned browser 2.16.0.

| Workload | Mode | Median ms | Full range ms | Median peak MiB | Full peak range MiB | n |
|---|---|---:|---:|---:|---:|---:|
| binary_trees | Code interpret | 106.335 | 105.540–110.372 | 31.535 | 31.047–31.785 | 15 |
| dictionary | Code interpret | 23.226 | 22.895–23.659 | 29.660 | 29.172–30.195 | 15 |
| sum_squares | Code cold run | 49.012 | 48.333–50.282 | 39.957 | 39.469–40.125 | 15 |
| sum_squares | Code interpret | 341.309 | 329.839–348.270 | 29.508 | 29.152–29.609 | 15 |
| sum_squares | Code warm run | 10.963 | 10.855–11.085 | 1.965 | 1.941–2.078 | 15 |
| sum_squares | Node/V8 | 39.534 | 37.656–41.392 | 49.961 | 49.621–49.996 | 15 |
| sum_squares | Python | 1122.949 | 1038.504–1297.932 | 10.289 | 10.156–10.312 | 15 |

## Compiler launches — separate from application RSS

These invocations use disabled compiler artifact caching; OS caches remain
warm. Cold `code run` includes its own build inside the source-run sample,
whereas these are explicit build commands. Compiler peak RSS must not appear
as application RAM. Waited descendants contribute a largest peak, not a sum
of simultaneously resident processes.

| Workload | Mode | Median ms | Full range ms | Median peak MiB | Full peak range MiB | n |
|---|---|---:|---:|---:|---:|---:|
| sum_squares | C O2 | 35.465 | 35.269–37.984 | 23.301 | 23.270–23.324 | 5 |
| sum_squares | C O3 | 35.973 | 35.449–36.735 | 23.895 | 23.812–24.012 | 5 |
| sum_squares | Code release | 38.322 | 38.142–38.442 | 40.281 | 40.137–40.422 | 5 |
| sum_squares | Rust O3 | 76.055 | 75.409–77.249 | 99.484 | 98.617–99.613 | 5 |

## Claim table for 216

| Candidate wording | Decision | Evidence / mandatory scope |
|---|---|---|
| “Native compilation can be competitive with C/Rust on numeric kernels.” | Use, scoped | This float64 loop, public v2.23.0, one recorded VM; show broader rows too |
| “Code runs at C/Rust speed.” | Reject as general statement | Five broader cases are 1.02–2.68× C time; full snapshots are 9.11× Rust Rc COW time |
| “Repeated CLI runs reuse the build.” | Use | Warm artifact inode/hash unchanged; warm 10.963 vs cold 49.012 ms on this program |
| “The interpreter is faster than JavaScript.” | Reject | Numeric interpretation 341.309 vs Node/V8 39.534 ms; browser engine not measured |
| “Automatic sharing avoids eager snapshot replication.” | Use, scoped | 13.984 MiB vs Rust Vec clones 262.070 MiB on the retained-matrix program; include its slower runtime and equivalent-COW rows |
| “Code memory management beats Rust.” | Reject | Typed Rust Rc COW uses 8.004 MiB and 36.760 ms; Code 13.984 MiB and 334.822 ms; representations differ |
| “Copy-on-write updates are 9× slower in Code.” | Reject as isolated-operation claim | This is complete-program time including initialization, scans and cleanup; no isolated COW microbenchmark was run |
| “The ABI slot is 16 bytes, so applications use 16 bytes per variable.” | Reject | ABI layout is not total retained memory, allocator overhead or host RSS; see Memory guide |
| “Cold-run/compiler RAM is the application's RAM.” | Reject | Different measurement groups; compiler/cold numeric peaks around 40 MiB, prebuilt numeric 1.355 MiB |
| “v2.23.0 is X% faster than the October source.” | Reject from this dataset | Different CPU/OS/toolchains, timers and source revisions; historical metadata is incomplete |
| “Less RSS means fewer allocations or zero pauses.” | Reject | Allocation counts and pause distributions were not measured |

216 should display a small scoped selection with links to all raw results,
representation choices, slower cases and the historical version boundary.
Avoid mixing October Java/Go numbers with this VM's new rows or projecting
native CLI results onto the browser interpreter. A dedicated-hardware rerun
and additional real applications would be needed for broader launch claims.

RSS interpretation follows the Linux man-pages for
[wait4](https://man7.org/linux/man-pages/man2/wait4.2.html) and
[getrusage](https://man7.org/linux/man-pages/man2/getrusage.2.html): it is resident
memory accounting, not an allocation counter or summed process-tree RSS.

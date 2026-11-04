# Public-release evidence for the Code website

Ticket 215 supplies facts for the later Performance page (216). This is a
measurement suite and source audit, not a language optimization. October files
under `language-comparison/` and `workloads/` remain unchanged.

## Reproduce

Run the manual **Verify public-release website evidence** GitHub workflow
(`.github/workflows/website-evidence.yml`). It downloads the actual Linux
v2.23.0 CLI/driver, prepares and validates every program, then samples serially
on one allowed logical CPU in a fresh Ubuntu 24.04 hosted VM. No tests or
profilers run in that measurement job. Other workflows use separate VMs.
Physical-host isolation and fixed cloud CPU frequency are not promised.

The artifact `public-v2.23.0-website-evidence` contains `report.json`, ordered
raw `samples.csv`, and the prepared binaries. The report records release/tag
identity, launcher/driver hashes, workload/harness hashes, artifact hashes,
actual commands, compiler/runtime versions, machine/OS/image identity, affinity,
sampling window, CPU pressure and a fingerprinted historical inventory. Compiler
samples have separate output paths: they cannot overwrite runtime artifacts.

For functional checks on a busy development machine, without publication timing:

```sh
python3 benchmarks/website-evidence/run.py \
  --code /absolute/path/to/code-v2.23.0-x86_64-linux/code \
  --output-dir /tmp/code-evidence-smoke --smoke-only
```

The output directory must be new. Smoke mode builds/checks all variants and
primes the native cache, but publishes no timing samples. On another quiet
Linux host, omit `--smoke-only` to reproduce sampling; the harness rejects a
dirty tracked source tree or detected concurrent builds/tests/profilers.
This report's measurements use the fresh hosted workflow. Requires GCC/cc, Rust, Python and
Node. Java and Go are intentionally absent from this selected rerun; their
October numbers must not be mixed into a new same-host ranking.

## What is measured

- **Prebuilt:** sum-squares, all five existing broader workloads, and snapshots.
  Application startup, computation, checks, cleanup and any program output are
  included; building is outside these runtime measurements.
- **Source:** numeric Code cold-cache run, valid warm-cache run, explicit
  interpretation, CPython and Node/V8. Interpretation is also sampled for trees
  and dictionary. Cold cache means an empty Code cache, not a cleared OS cache.
  The warm executable's inode/hash must stay unchanged; access refreshes mtime.
- **Compile:** numeric Code `build --release`, GCC O2/O3 and Rust O3. Their
  compiler processes' RSS belongs to this group, not application-memory tables.

Every target checks its answer, including before sampling. Each runtime mode
gets 15 fresh launches; compiler modes get five. Order is deterministically
shuffled within each repetition (seed 20261104). Native cold samples verify a
new executable was produced; they cannot silently fall back to interpretation.
The small numeric source uses the optimized-first native path; larger-program
background optimization is not part of this launch comparison.

The existing `peak.c` C wrapper times fork/exec/wait with a monotonic clock,
excluding Python-harness/wrapper startup. Linux `wait4` peak RSS is in KiB and
includes the maximum inherited/waited-descendant process peak; it does not sum
simultaneously resident processes, count allocations or measure retained heap.
Cold runs include compiler descendants and transient startup before exec. Tiny
processes can hit accounting/startup floors. Summary includes median, full
range, median absolute deviation and zero-RSS counts; raw samples stay visible.
These are descriptive observations, not statistical proof of a language ranking.

## Source/representation audit

| Case | Code | C | Rust | Limits |
|---|---|---|---|---|
| Sum squares | Float64 scalar loop | Float64 loop, GCC O2/O3 | Float64 loop, Rust O3 | 10M logical iterations; compiler transformations are allowed; not an executed-instruction count |
| Spectral norm | Value arrays, new result per pass, 60 Newton steps at end | Reused static double arrays, `sqrt` | Fresh typed vectors, `sqrt` | Same final numeric bound, different allocation strategy and final square-root implementation |
| Trees | Nested value objects and explicit walk | Separately allocated two-pointer nodes, explicit release walk | Boxed node pairs, explicit count walk, Rust drop | Same checked node count, different object layout, ownership and cleanup |
| Text growth | Automatic value-string growth | C buffer + integer formatting | String growth + formatting | Same 108,890-byte ASCII result, runtime formatting/allocation differ |
| Dictionary | Ordered value object + computed-key merges | Fixed open-addressed string table | Standard HashMap with per-process hasher state | Same 500 counters/50 passes; not identical lookup algorithms or key-allocation costs |
| Points | List of dynamic four-field objects | Typed four-double struct array | Typed four-double structs in Vec | Same 1,001,398 checksum, representation and bounds/type checks differ |

The current checked-in broader source files are not modified by this suite;
their bytes need not match the October revisions. C/Rust print their checked
checksum/length where written, while Code asserts silently. That output is
included. Very short rows are therefore complete-program observations, not a
controlled estimate of language overhead. Flags also differ: Code's public
`--release` uses its LLVM O2 pipeline; C uses GCC O2 (O3 additionally for the
numeric loop), Rust O3; C/Rust select the measurement CPU's native target.
The public CLI source pins the LLVM 17 family; the exact statically linked
LLVM patch is not inferred from an unrelated host `llvm-config` executable.

### Retained snapshots: matching value semantics, several representations

All five snapshot variants initialize a 128×4096 matrix of float64 values,
retain 64 pre-update versions and increment one diagonal cell per update. All
65 matrices remain available until every full checksum is checked against
`n*(n-1)/2 + version`. They produce no output. Initialization, updates, retaining,
validation and cleanup all count toward runtime and peak RSS.

Code uses ordinary value assignment and part updates. Rust's ordinary
`Vec<Vec<f64>>::clone()` and the eager C version copy every cell for each saved
version. The equivalent-sharing baselines are Rust `Rc<Vec<Rc<Vec<f64>>>>`
with `Rc::make_mut` at both levels, and C with explicit non-atomic counts and
changed-path copying. Both C/Rust alternatives retain dense 8-byte float64
cells; Code's generic container slots, headers, growth capacity and reference
counts have their own costs. C's eager variant retains the small bookkeeping
fields in its row/matrix structs but shares no row buffers.

A win against eager deep clones cannot establish superiority over Rust or C:
equivalent COW variants are first-class results, including when they are faster
or use less RSS. This case demonstrates a particular snapshot strategy, not
universal application RAM, allocation rate or collection-pause behavior.

## Historical inventory and claim gate

`report.json` fingerprints every JSON/CSV result in the two original suites,
retains available metadata and links paired reports. Missing metadata is not
reconstructed from checkout mtimes or the current compiler.

| Canonical archive | Recorded time/revision | Meaning for the website |
|---|---|---|
| `language-comparison/results-before-exec.*` | 2026-10-14 UTC, `1ed460e`, Code version string 2.16.0 | Development baseline, not a measurement of the public 2.16.0 or 2.23.0 release bundle |
| `language-comparison/results.*` | 2026-10-14 UTC, `e539e6a`, development launcher/cache | Historical numeric/source/build/RSS data; exact binary hashes and LLVM patch not recorded |
| `language-comparison/reruns/2026-10-16/results.*` | 2026-10-15 22:55 UTC (16 October local), `3171efb` | Later development interpreter/launcher, not a release comparison on today's VM |
| `workloads/results.*` | 2026-10-15 UTC, `e897b40` | Fifteen-launch prebuilt timings; Python supervises the wall timer, no RSS, incomplete toolchain/machine/binary identity metadata |

Other `interpreter-*results.json` files are focused optimization experiments;
their before/after revisions, hashes and declared timing conditions are retained
where present. They do not become public-release language rankings. The live
Compare page's October date/version and mixed numeric/broader evidence must be
revisited in 216. Different hosts, supervisors, compilers and source revisions
prevent interpreting this rerun as a release-to-release speedup percentage.

Final raw results and a concrete claim/evidence table will be linked here after
the fresh hosted measurement completes. The live website is not rewritten by
215; 216 consumes the verified evidence.

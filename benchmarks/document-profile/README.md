# Retained document histories: instruction diagnosis

Task 237, continuing 126 after the public v2.23.2 release and website alignment.
Measured locally 6 November 2026, 08:20:50–08:23:05 UTC. This is a **profile,
not an optimization result or a new C/Rust speed ranking**.

[Full identities and counts](results/2026-11-06-public-2.23.2/report.json).
All eight generated programs, raw Callgrind files, annotations and execution/
leak-check logs are retained beside that report. No program/result was dropped.

## Finding and next candidate

The unchanged 1 MiB document program executes **5,126,111,052 instructions**.
Self instructions in `code_slice` account for **70.70%**, and `code_length_of`
for **23.56%**: **94.26% combined**. At 64 KiB the corresponding combined
share is **94.08%**. These are instruction shares, not elapsed-time shares,
and Callgrind does not establish memory-bandwidth or allocator wall-time shares.

Source inspection explains a concrete redundant operation to investigate:

- `code_slice` first scans the complete UTF-8 input to count characters. It
  then walks from the start again for each bound with `char_offset`, before
  allocating and copying the selected bytes. Even a short prefix slice pays
  the initial complete-input scan.
- A tail ending at index-context `length` also calls `code_length_of`, which
  performs its own complete character-count scan before `code_slice` starts.
- Each of 64 edits takes two slices. Checking histories constructs another
  64 expected tails. These are existing program operations, not new probes
  introduced into the original benchmark.

**Recommended next experiment:** locate both slice boundaries in one UTF-8
walk, stopping when the requested upper boundary or the terminator is reached,
instead of counting the entire input and restarting. Keep the existing
`code_length_of` path initially; this isolates a runtime change from compiler
fusion, character-count caches, storage changes or new language features.
This is a hypothesis; **no speedup or memory improvement is established here**.

Required preservation checks include multibyte Unicode scalar boundaries;
negative, fractional, reversed, empty, huge and out-of-range bounds; both
open/closed range syntaxes; the current behavior for non-finite bounds;
wrong-kind diagnostics; independent shared/nested snapshots; native/interpreter/
compiled-WASM parity; and output/input aliasing. Do not cast an unclamped huge
double directly to a signed integer or reinterpret character positions as bytes.
Do not remove validation from the measured full program to manufacture a gain.
Runtime/vendor copies and ABI 2 must remain synchronized and unchanged in layout.

Any implementation needs its own frozen-source before/after comparison, at
least a 10% target elapsed-time improvement, no material broader regressions,
all samples retained, full workspace and relevant application/WASM checks.
The original public website numbers and graphs are unaffected by this profile.

## Cumulative diagnostic programs

| Program | 64 KiB instructions | 1 MiB instructions |
|---|---:|---:|
| Construction and cleanup | 272,408 | 2,394,828 |
| Construction, retained edits and cleanup | 194,021,878 | 3,095,150,728 |
| Edits plus expected-value construction/structural checks, without text equality | 320,318,661 | 5,113,821,308 |
| Original complete program | 321,116,248 | 5,126,111,052 |

`construction` ends before creating the snapshot list. `edits` ends after
retaining the final version. `edits-expected` removes only the text-equality
assertion from the original full program, keeping its expected-value construction,
flags, section counts and history count. The two `full` sources are byte-for-byte
the original v2.23.2 website workload sources, independently hash-checked against
the retained hosted report. All programs include startup and cleanup.

For 1 MiB, adding expected-value construction and structural checks increases
the cumulative count by 2,018,670,580 instructions (39.38% of the full total).
Adding the equality assertion changes the count by 12,289,744 (0.24%). These
differences help choose where to investigate; they are **not exact isolated
phase measurements**. Omitting statements can change generated code, ownership,
allocation lifetimes and cleanup. In particular, a small instruction share for
equality does not prove its elapsed-time cost is negligible. Construction-only
has a much shorter lifetime and retains no histories.

The public one-host comparison measured complete document-1m execution at
510.264 ms for Code and 41.306 ms for Rust explicit sharing. Those wall times
include construction, edits, building expectations, all checks and cleanup.
They must not be described as an isolated COW/update cost, and this local
instruction experiment does not replace or combine timings with that host.
All 65 retained snapshots are checked by the original complete programs.

## Reproduce and verify

Use the actual delivered Linux x86_64 v2.23.2 launcher and adjacent driver.
The runner checks both exact SHA-256 values against the committed release
manifest; a developer binary bearing the same version is insufficient.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 benchmarks/document-profile/run.py \
  --code /path/to/public-v2.23.2/code \
  --annotate /path/to/callgrind_annotate \
  --output-dir /tmp/new-document-profile

PYTHONDONTWRITEBYTECODE=1 python3 benchmarks/document-profile/verify.py \
  benchmarks/document-profile/results/2026-11-06-public-2.23.2 \
  --artifacts /tmp/new-document-profile
```

Omit `--artifacts` to verify committed profiles/source hashes without the native
executables. The executable check is for the original retained artifact directory;
fresh builds can differ with toolchain, paths and environment. The first command
requires an unused directory. Programs compile and pass silent-success and native
leak checks before profiling serially, pinned to one allowed CPU. Callgrind
records only Ir; cache/branch simulation is disabled. Local machine activity can
affect elapsed time, which is not sampled or published by this runner.

The retained machine is Intel i7-5930K / Debian x86_64, Valgrind 3.24.0, CPU 0;
exact platform, release/tool/source/generated-program/native artifact identities
and commands are in the report. The source revision records the repository
baseline before the new profiling script was committed; its exact script hash
is separately included and verified. No runtime, compiler, ABI, interpreter,
module, website or release file was changed by this investigation.


## Validation

Full `cargo test --workspace`: **341 passed, zero failed, seven ignored**,
69 summaries. All eight programs passed silent-success/native leak checks;
both original full sources match the published benchmark bytes. Independent
verification checks every retained source/raw/annotation hash, instruction
summary and original executable identity. No failing test, runtime crash or
mode disagreement occurred.

[Validation identity](results/2026-11-06-public-2.23.2/validation.json) records
the complete workspace-log hash. The small analysis helper initially failed
to parse leading spaces in annotated function lines; correcting its regex
produced the percentages above, without rerunning or excluding a profile.
The independent profile/source/artifact verifier passed on its first run.

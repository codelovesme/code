# Retained histories: exploratory pilot (231)

This pre-registered pilot investigates useful value-semantics workloads. It
is **not a website update or a universal language ranking**. All configurations,
variants and results are kept, including losses. Old benchmark files remain
unchanged. New host numbers must not be merged into the old host's charts.

## Fixed cases

Each retains and verifies all 65 independent versions until cleanup:

| Case | Sections | Text per section | Change per revision |
|---|---:|---:|---|
| config_4k | 64 | 4096 ASCII bytes | Set a different section's flag |
| config_64k | 64 | 65536 ASCII bytes | Set a different section's flag |
| document_64k | 1 | 65536 ASCII bytes | Replace the next initial `a` with `b` |
| document_1m | 1 | 1048576 ASCII bytes | Replace the next initial `a` with `b` |

Variants: Code automatic value sharing; C eager copies and explicit reference
counting/COW; Rust ordinary Vec/Section/String clones and explicit Rc/make_mut
sharing. Input contents, flags, final histories and full-value checks match.
All payloads are ASCII. There is no network, JSON parser or module I/O here.

This contrasts an expected sharing-friendly use case with one that must copy
changed text. It does not predetermine that Code wins. Typed explicit-sharing
baselines are first-class results, not omitted when faster or leaner. Eager
copies are independent-version baselines, not claims about optimized C/Rust.

## Implementation and measurement boundary

- Code assigns values and changes nested fields. Initial repeated text is made
  by doubling. C uses initialized byte buffers; Rust uses String::repeat.
- Shared C counts roots/sections/text; shared Rust uses Rc at the same levels.
  A sparse flag edit can share payloads. Eager variants deeply copy payloads.
- Public Code v2.23.0 does not support string-part assignment. Document edits
  use its existing slices/concatenation, while C edits its private byte buffer
  and Rust uses replace_range after detaching. These have different allocation
  and validation costs; they are matching outcomes, not identical instructions.
- All versions' flags and complete text are checked. Code document validation
  constructs each expected prefix/tail value; C/Rust reuse a mutable expected
  buffer. Shared immutable text can use pointer-identity shortcuts. Validation
  is included, so timing is **not an isolated update/COW microbenchmark**.
- Code generic objects/strings and typed C/Rust structures have different
  metadata, allocation and layout. Startup, construction, retaining, edits,
  all checks and cleanup are included. These are process-level measurements.
- Source/native compilation is completed before timing. Code release, GCC O2
  with native CPU targeting, and Rust O3/native targeting are recorded. The
  fixed public Code launcher/driver hashes are verified, not inferred from a
  developer binary's version string. No source-only feature is required.
- Use the manual `Measure retained-history benchmark pilot` workflow. It runs
  on a fresh Ubuntu VM, pins sampling to one allowed logical CPU and samples
  20 variants serially in seeded shuffled order, 15 times each (300 launches).
  Physical-host isolation/fixed frequency is not promised. No compiler/test/
  profiler is allowed during sampling. RSS uses the existing wait4 wrapper:
  largest resident process peak, not live heap or summed tree/allocations.
- The report records every source/generated program, build command and binary
  hash, release/tool/machine identity, sampling window and pressure. Every
  sample stays in CSV/JSON; min/max/median/MAD and zero-RSS counts are retained.

Local functional validation (no timing samples):

```sh
PYTHONDONTWRITEBYTECODE=1 python3 benchmarks/history-pilot/run.py \
  --code /path/to/public-v2.23.0/code --output-dir /tmp/new-pilot-smoke --smoke-only
```

Output directories must be new. Timing mode is restricted to the fresh hosted
workflow and a clean tracked checkout. Existing published-memory evidence
already shows lower Code peak RSS in dictionary/points/spectral-norm on that
specific short-program set; small process-floor differences alone do not prove
lower application heap use. This pilot tests a larger retained-data regime.


## Recorded outcome

[The complete 5 November report](RESULTS.md) retains all 300 samples from
[hosted run 42099367294](https://github.com/codelovesme/code/actions/runs/42099367294).
Sparse configuration histories favor automatic Code sharing over eager full
copies; explicitly shared C/Rust remain faster. Document revisions are a
clear Code slowdown and are retained as the counterexample. No website
comparison/claim was changed by this pilot.

Verify retained sample/statistic integrity without rerunning the benchmark:

```sh
python3 benchmarks/history-pilot/verify.py benchmarks/history-pilot/results/2026-11-05-v2.23.0
# Add --artifacts /path/to/downloaded-bundle to check its prepared source/binary bytes too.
```

# Rejected experiment — no runtime change retained

[Complete negative result](RESULTS.md): ASCII document-1m improves 38.74%,
but Unicode document-1m regresses 28.99%, failing the preregistered gate.
PR 81 is closed unmerged; main retains 239. The source experiment is preserved
at c0eb1676ebba69b6486d48c3a5e0c40fd03362e6 / ascii-slice-seek-240.
The original workflow is archived as [rejected-workflow.yml](rejected-workflow.yml);
it is not an active workflow on main. Its 400-slice input is archived as
[regression.code](regression.code), not added to the public gallery or core tests.

Verify all retained samples and downloaded artifact bytes:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 benchmarks/ascii-slice/verify.py \
  benchmarks/ascii-slice/results/2026-11-06-source \
  --artifacts /path/to/downloaded-ascii-slice-seek
```

The following is the original candidate protocol, preserved to show its
predeclared controls and rejection criteria; it does not describe an active
runtime patch. Reproduction needs the frozen source branch, not main's runtime.

---

# Bounded native ASCII slice seeking — preregistered experiment

Task 240 continues 126 after 239. In the matched post-239 document-1m local
profile, `code_slice` accounts for 74.48% of self instructions. These are
instruction shares, not elapsed-time percentages.

Frozen baseline `af3430d26dc7d577feffd756c3e9c444f57eec0b`, Code 2.24.0.
The candidate changes only finite native string-slice seeking in `runtime.c`
and its identical vendored copy. No compiler/interpreter, ABI/storage/header,
cache state, syntax, website or package release change is included.

## Candidate and boundaries

The native POSIX helper takes bounded `strnlen` lookahead: at most 256 bytes,
never more than the remaining character budget, and stopping at the first
NUL. Only that proven readable span is inspected with memcpy word loads.
ASCII groups advance eight characters at once. At the first non-ASCII byte,
the original scalar UTF-8 walk handles the rest. There is no unaligned cast,
ISA requirement, cache flag or whole-input scan for a small prefix.

Slice starts and relative ends use the helper after the existing finite-bound
conversion/clamping. Wrong-kind diagnostics, array and non-finite paths remain.
The helper is excluded under CODE_WASM; compiled WASM retains its original
finite-bound loops and gains no host import.

## Fixed workload set and acceptance

The original 17 task-239 programs remain byte-for-byte unchanged. Two added
controls, document_64k_unicode and document_1m_unicode, use the identical
retained-history algorithm/checks with initial payload `é` instead of `a`.
Their 65 independent histories retain 64 edits, replacing one scalar per
revision. Initial payloads have 65,536 / 1,048,576 scalars, twice as many UTF-8
bytes; replacing é with b reduces bytes while preserving scalar count. These
are internal regression controls, not new public comparison charts.

Fresh hosted VM; identical LLVM 17/Rust toolchains and separate before/after
Cargo output trees. Both frozen compilers identify as 2.24.0 with distinct
recorded hashes. Prepare/check every executable before serial sampling on one
allowed CPU: **19 workloads / 38 entries / 570 samples**, 15 seeded shuffled
launches per entry, all retained. No concurrent compiler/test/profiler during
timing; physical isolation/fixed frequency are not guaranteed.

Startup, construction, algorithm, complete original checks and cleanup are
included. Compilation and Python/wrapper startup are excluded. OS caches stay
warm. RSS is wait4 largest process-resident peak, not useful heap, allocation
count or a summed process tree. Retain all medians/ranges/MAD/zero-RSS counts,
tool/compiler/environment/source/native/wrapper identities and raw rows.

Retain only with **>=10% less complete original document-1m median time** and
no material regression in **any** of the other 18 cases, including Unicode:
**>=10% slower AND absolute increase > max(0.2 ms, 3×before MAD, 3×after MAD)**.
Otherwise revert the candidate and retain evidence. No control or unfavorable
sample is removed. Do not combine these source timings with previous hosts or
public-release C/Rust rankings; no new public benchmark result is implied.

Local functional preparation, with no timing samples:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 benchmarks/ascii-slice/run.py \
  --before /path/to/frozen-before --after /path/to/frozen-after \
  --output-dir /tmp/new-ascii-slice-smoke --smoke-only
```

Timing requires the hosted workflow and a clean tracked checkout. Independently
verify all 570 sample rows, statistics and source/native/wrapper artifacts.

## Safety and parity

The new language regression checks 400 slices against an independent 801-scalar
list, including fractions, negative/huge bounds, long ASCII runs and Unicode
transitions at scalar indices 7/8, 255/256/257 and 511/512/513. Existing 238/239
regressions cover other bound/type/alias/empty/character-count cases. Execute
interpreter/native dev+release/compiled-WASM dev+release and source-WASM checks;
the existing stress_ prefix keeps the public gallery unchanged.

The actual-runtime C `guard.c` compares the original scalar walk at lengths
0..529, nine ASCII/non-ASCII transitions and eight bounds including 1e200:
**305,280 cases / 610,560 calls per run**. It checks borrowed and owned
output/input aliases. A protected page follows the final readable NUL, testing
every word/chunk tail and alignment, including raw partial UTF-8 byte patterns
with legacy scalar behavior. Baseline/candidate address/undefined/leak sanitizer
runs and candidate optimized native execution must pass:

```sh
cc -O1 -g -fsanitize=address,undefined -fno-omit-frame-pointer \
  -ffunction-sections -fdata-sections -Wl,--gc-sections \
  -Isrc benchmarks/ascii-slice/guard.c -o /tmp/ascii-slice-guard -lm -ldl -lpthread
ASAN_OPTIONS=detect_leaks=1:halt_on_error=1 UBSAN_OPTIONS=halt_on_error=1 \
  CODE_CHECK_LEAKS=1 /tmp/ascii-slice-guard
```

Also require full workspace, runtime/vendor identity, clippy/format, 178 actual
source-engine examples, applicable frozen app fixtures and Aquarium size
(>5% needs explanation). Both PR CI jobs must pass before merging. Retain raw
profiles and every preparation failure/limitation. No release/tag, public site
update or live application restart is part of this experiment.

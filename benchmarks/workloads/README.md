# Workloads: code against C and Rust beyond one loop

`language-comparison/` measures one numeric loop, where compiled `code` now
matches C (ticket 113). These five measure what real programs spend their
time on — lists, nested objects, text, computed keys — with the same
algorithm in `code`, C and Rust. Every program checks its own answer.

| Workload | What it exercises |
|---|---|
| `spectral_norm` | numbers read from lists, a list built per pass (Benchmarks Game) |
| `binary_trees` | nested objects built bottom-up and walked with a stack |
| `string_build` | a string grown one piece at a time |
| `dictionary` | an object updated under computed keys |
| `points` | a list of `{ x, y, vx, vy }` moved step by step (a game frame) |

The language has no recursive handler calls or built-in `%`, `floor` or
`sqrt`, so these programs use iterative steps. Part assignments and growth
can update uniquely held containers in place; shared values keep their old
contents. C and Rust use their idiomatic mutable representations. Layout,
sharing, allocation, and lookup costs are part of what is measured.

```sh
CODE_BIN=/path/to/code benchmarks/workloads/run.py [--runs 5] [workload ...]
```

The runner pins launches to one allowed logical CPU unless `--no-pin` is
given, shuffles the language order deterministically on each repetition,
and records raw launch times in `results.csv` and medians/ratios in
`results.json`. Startup is included and can dominate the shortest programs.
Use `--output-dir` to keep an experimental run separate from the published
results.

## Interpreter object-owned indices (2026-10-16, unreleased)

Against main `6dfc002`, implementation `09861d3`: ordered object entries now
own optional lazy index metadata. Objects below 32 fields keep linear scans;
larger objects index first positions on demand. Value-only writes preserve the
index, appends extend it, arbitrary mutable entry access invalidates it, and
copy-on-write clones start with a fresh index. Duplicate reads still select the
first occurrence; merges still replace every matching left occurrence with
the first matching right value and preserve order/new duplicates. No weak
owners, pointer-identity cache or new unsafe code.

Release-built full CLIs, explicit `run --interpret`, CPU 0, serial shuffled
fresh launches including startup/parsing/lowering. Original workload sources
and assertions are unchanged. No concurrent builds/tests/profiling during
timing. The final rebuilt binary was remeasured after documentation edits
changed its hash. Medians of 15 launches per mode, except full spectral (three):

| Workload | Before | After | Elapsed-time change |
|---|---:|---:|---:|
| dictionary | 67.546 ms | 23.275 ms | -65.5% |
| sum_squares | 327.660 ms | 328.732 ms | +0.3% |
| binary_trees | 102.869 ms | 104.192 ms | +1.3% |
| string_build | 12.996 ms | 12.626 ms | -2.8% |
| points | 249.918 ms | 250.317 ms | +0.2% |
| spectral_norm | 12.651 s | 12.381 s | -2.1% |

**Dictionary uses 65.5% less time (2.90x faster)**, above the 10% target gate.
The small tree cost is reported rather than called zero. Other small changes
are not additional target wins. Raw data and binary hashes:
`interpreter-dictionary-index-results.json` and
`interpreter-dictionary-index-spectral-results.json`.

Full unchanged dictionary Callgrind: **609,823,480 -> 151,841,840 instructions
(-75.1%)**. Before: merge_object_into 24.53%, index_operand 9.46%, and an unnamed
libc routine called by the key scans 43.87% of self instructions. These are
instruction shares/counts, not CPU-time shares. Profiles and preliminary
variants remain in `/var/tmp/interpret-next/dictionary-index/`.

Tradeoffs: a native Rust layout probe gives ObjectFields 32 bytes versus the
previous Vec's 24 bytes (8 bytes per object, including small objects); indexed
objects additionally allocate a table and cloned key strings. First access to
a large object pays to build that table. Rust embedders constructing objects
from a vector use `Value::Object(Rc::new(entries.into()))`; iterator collect
can infer ObjectFields directly. This is a Rust source API adaptation; the
language's six kinds, order, JSON and native module C ABI are unchanged.

Local validation passes: workspace debug/release builds, `cargo test --workspace`,
clippy with warnings denied, Rust/fixture formatting, wasm32 build, 145
playground examples and all 70 app
fixtures in interpreter/dev/release. Tests cover aliases, duplicate ordering,
same-FNV distinct names, external Vec-style swaps/renames/removals, 256 random
wide merges checked against generic addition, and iterative teardown through
4,000 indexed object levels. Fresh Aquarium release wasm is byte-identical:
901,255 bytes, SHA-256
`d923e206e787ed895e9718bb94a151ad1d68e80931d7c0aec309ef187d74b36e`.

## Expression temporary buffers (2026-10-16, unreleased)

Against main `02a8c25`: successful numeric expressions keep their generic
fallback in a separate expression program, so its boxed registers are only
prepared if a guard fails. Numeric lowering reuses dead intermediate registers
while preserving left-to-right evaluation and keeping the left operand alive
until the right is read. Both banks remain invocation-local; no unsafe code,
shared scratch storage, value layout, scalar VM or module ABI changes.

Release-built full CLIs, explicit `run --interpret`, CPU 0, serial shuffled
fresh launches with startup/parsing/lowering included. Original workload sources
and assertions are unchanged. No builds/tests/profiling ran during timing.
Medians of 15 launches per mode, except full spectral (three per mode):

| Workload | Before | After | Elapsed-time change |
|---|---:|---:|---:|
| spectral_norm | 14.887 s | 12.677 s | -14.8% |
| sum_squares | 327.073 ms | 326.160 ms | -0.3% |
| binary_trees | 101.775 ms | 102.128 ms | +0.3% |
| dictionary | 66.271 ms | 67.564 ms | +2.0% |
| string_build | 12.991 ms | 13.061 ms | +0.5% |
| points | 258.961 ms | 248.031 ms | -4.2% |

**Spectral uses 14.8% less time (1.17x faster)**, above the 10% target gate.
The small dictionary cost is reported rather than called zero; other small
differences are not additional target wins. Raw samples and binary hashes:
`interpreter-buffer-results.json` and
`interpreter-buffer-spectral-results.json`.

The isolated fallback-only variant measured **14.961 -> 13.575 s (9.3%)**,
below the gate and not retained alone. Its three-pair raw samples are in
`interpreter-buffer-fallback-only-results.json`; the final result above also
includes numeric-register reuse, which removes the second temporary bank's
repeated heap allocation.

For explanation only, the reduced N=100 spectral instruction profile (assertion
1.274 < result < 1.275) changes **1,453,678,820 -> 1,185,779,226 instructions
(-18.4%)**. The before profile is reused from the preceding experiment's exact
same frozen binary (SHA-256 `77b002bd...`); this is not a new wall-time pair.
Expression value-bank preparation calls fall from 400,100 to zero, and its
numeric-bank reserve calls from 400,000 to zero. Other allocations still occur.
Scalar Code::run and numeric Read::read self-instruction counts are unchanged.
Instruction counts/shares are not CPU-time measurements. Profiles and diagnostic
logs are retained locally in `/var/tmp/interpret-next/buffers/`.

Aquarium release wasm rebuilt from the same current source with both CLIs is
byte-identical: **901,255 bytes**, SHA-256
`d923e206e787ed895e9718bb94a151ad1d68e80931d7c0aec309ef187d74b36e`.
Both builds write only to scratch output directories.

Local validation: full workspace debug/release builds and tests, clippy with
warnings denied, Rust/fixture formatting, wasm32 code-wasm build and all 145
playground/landing examples pass. Regression cases include long/balanced trees,
nested computed indices, late null guards, repeat calls preserving old answers,
and host replacements requiring string concatenation in original operand order.

## Guarded numeric expression reads (2026-10-16, unreleased)

Against main `863b6b3`, implementation `85888f6`: release-built CLIs with
explicit `run --interpret`, serial shuffled fresh launches, CPU 0. Startup,
parsing and lowering are included; no concurrent builds/tests/profiling ran.
Original sources and result assertions are unchanged. Medians of 15 launches
per mode except the long spectral target (three launches per mode).

| Workload | Before | After | Elapsed-time change |
|---|---:|---:|---:|
| spectral_norm | 20.961 s | 15.015 s | -28.4% |
| sum_squares | 328.170 ms | 328.091 ms | -0.0% |
| binary_trees | 101.878 ms | 102.180 ms | +0.3% |
| dictionary | 67.081 ms | 67.388 ms | +0.5% |
| string_build | 12.911 ms | 13.006 ms | +0.7% |
| points | 253.902 ms | 259.520 ms | +2.2% |

**Spectral_norm uses 28.4% less time (1.40x faster)**. Other small changes are
not claimed as gains. The scalar numeric loop retains its previous speed.
A reduced N=100 instruction profile, separate from full-size timing, fell
**1,956,963,615 -> 1,453,678,820 instructions (25.7%)**. Before the change,
Expression::run accounted for 51.86% of instruction references, with additional
value/drop/vector/allocation work. This is not a CPU-time percentage.

Numeric inference can now select guarded reads from boxed shared/handler
bindings and from a named numeric list at a numeric index. The values remain
in their existing representation. These reads feed numeric expression registers;
a guard miss takes the original expression path for type/null/bounds/error
semantics. Reads hold no data across callbacks. The block VM's scalar runtime
instructions and Address variants stay unchanged. Bare borrowed reads and a
single operation on boxed numbers keep their cheap original paths rather than
round-tripping through numeric registers. There is no storage/ABI change, JIT,
whole-loop recognition, field-read optimization or floating-point reassociation.

An earlier broad prototype reached 6.98 s on spectral_norm but regressed the
scalar loop by 54.8% and dictionary by 18.1%; **that is rejected evidence, not
a retained gain**. Dedicated reads, preserving the scalar block VM, and avoiding
over-specialization removed those regressions. The first new fixture also used
unsupported `1e30` syntax; spelling out the integer repaired its all-mode parse
failure. No runtime crash or answer disagreement occurred.

Full workspace tests, clippy with warnings denied, Rust/fixture formatting,
debug/release builds and the wasm32 interpreter build passed. Host callback
coverage checks null, NaN/infinite/negative/fractional/large indices, container
replacement, dynamic string addition, failed writes, and shared boxed bindings.
The expanded fixtures also cover aliases and evaluation/error order.
Both numeric fixtures passed in the fresh wasm interpreter, all 145 playground
examples and all 70 app fixtures in three modes passed, and both PR CI jobs
passed. [PR #53](https://github.com/codelovesme/code/pull/53) merged as `1558212`.

Raw samples, hashes and profile totals:
[short workloads](interpreter-numeric-reads-results.json),
[full spectral target](interpreter-numeric-reads-spectral-results.json).

The preceding comparison/group fusion trial was reverted: **328.491 ->
310.896 ms, 5.36% less time**, below the 10% retention gate.
[Rejected trial samples](interpreter-compare-stopped-results.json).
No release was made.

## Field-read experiment stopped (2026-10-16)

After PR #53, a separate candidate combined `object.x` and `records[i].x`
with a checked numeric read in the expression VM. Fifteen serial shuffled
launches per mode, CPU 0, unchanged sources:

| Workload | Before | Trial | Elapsed-time change |
|---|---:|---:|---:|
| points | 257.200 ms | 221.553 ms | -13.9% |
| dictionary | 66.678 ms | 71.618 ms | +7.4% |
| sum_squares | 325.612 ms | 325.659 ms | +0.0% |
| binary_trees | 102.200 ms | 101.482 ms | -0.7% |
| string_build | 12.856 ms | 13.002 ms | +1.1% |
| spectral_norm (3 launches per mode) | 14.931 s | 15.101 s | +1.1% |

**Reverted because the dictionary regression outweighed keeping this narrow
candidate under the non-regression policy.** The points gain is rejected
trial evidence, not the current implementation's speed. Its full-size points
instruction count fell 1,891,401,617 -> 1,724,694,417 (8.8%). No object index,
storage change, release or deployment was involved.

Focused host-replacement and numeric/shapes/set_parts/loop fixtures passed,
including missing/null fields, invalid indices, aliases, duplicate-field
first-match semantics and string fallback. No crash or mode disagreement
occurred. Full candidate workspace/wasm/app validation was not run after the
performance stop. No field code, fixture or implementation PR remains;
restored main keeps the verified PR #53 improvement above.

[Rejected short-workload samples](interpreter-field-stopped-results.json) and
[non-target spectral samples](interpreter-field-stopped-spectral-results.json).
The local patch, extra fixture and frozen binaries are preserved under
`/var/tmp/interpret-next/fields/` for a future independently profiled design.

## Interpreter numeric instruction fusion (2026-10-16, unreleased)

Release-built interpreter CLIs, main `ac6cb6e` versus numeric arithmetic/write
fusion: 15 serial fresh-process launches per mode, deterministic shuffled order,
pinned to logical CPU 0. Parsing, lowering and startup are included. There were
no concurrent builds, tests or profiling during timing; each program checks its
own answer.

| Workload | Before | After | Elapsed-time change |
|---|---:|---:|---:|
| sum_squares | 405.81 ms | 328.96 ms | -18.9% |
| binary_trees | 102.26 ms | 101.56 ms | -0.7% |
| dictionary | 68.10 ms | 66.77 ms | -2.0% |
| string_build | 13.35 ms | 12.97 ms | -2.9% |
| points | 251.53 ms | 252.47 ms | +0.4% |
| spectral_norm (3 launches per mode) | 21.09 s | 21.15 s | +0.3% |

**The numeric loop uses 18.9% less elapsed time (1.23× faster)**. Small changes
on other workloads are not claimed as gains. Callgrind counted
**5,358,074,552 → 4,388,067,804 instructions, down 18.1%**. Before the change,
95.0% of instruction references were in bytecode execution and 4.85% in scope
cleanup. These are instruction counts, not sampled CPU-time percentages.

The final binary numeric operation writes directly to its destination instead
of a temporary register followed by a separate store. With leaf operands its
fallback metadata is carried in that same instruction, removing the separate
group instruction too: `i = i + 1` takes one dispatch instead of three. Nested
operands keep their existing evaluation order, intermediate rounding, checked
fallback and error path. Arithmetic succeeds before the destination is changed;
loop delivery and trace boundaries are unchanged. No whole-loop recognition,
algorithm shortcut, JIT, native ABI or C-runtime change is involved.

Raw samples, binary hashes and profile totals:
[interpreter-numeric-results.json](interpreter-numeric-results.json).
The longer non-target check is recorded in
[interpreter-numeric-spectral-results.json](interpreter-numeric-spectral-results.json).

Full workspace tests, clippy with warnings denied, Rust/fixture formatting,
the wasm32 interpreter build, the expanded numeric fixture in that fresh engine,
all 144 playground examples and all 70 application fixtures in
interpret/dev/release modes passed. Both PR CI jobs passed;
[PR #52](https://github.com/codelovesme/code/pull/52) merged as `65975d5`.
Temporary app-test module links were removed. Regression coverage includes dynamic
host replacements on either operand, nested-expression fallback, noncommutative
operations, rounding and failed writes leaving the destination unchanged.

The first full run reached the launcher tests but failed with `StorageFull` /
`No space left on device`: the 32 GiB `/tmp` tmpfs was full. Moving temporary
output under the home directory then made two module-install tests discover the
existing ancestor `~/.code` directory. The final complete run passed with
`TMPDIR=/var/tmp/code-tests-numeric-cdlvsm`, outside both the full tmpfs and the
home project search path. Neither environment failure required a source change.
No release was made.

## Interpreter object growth (2026-10-16, unreleased)

Measured independently after list growth, against main `e680e1a`. Release-built
CLIs run each source with explicit `--interpret`: 15 serial fresh-process launches
per mode, deterministic shuffled before/after order, logical CPU 0.

| Workload | Before | After | Elapsed-time change |
|---|---:|---:|---:|
| binary_trees | 103.11 ms | 103.14 ms | +0.0% |
| dictionary | 750.66 ms | 69.49 ms | -90.7% |
| string_build | 13.44 ms | 13.51 ms | +0.5% |
| points | 260.55 ms | 252.80 ms | -3.0% |
| sum_squares | 405.71 ms | 402.47 ms | -0.8% |

**Dictionary is 10.8× faster (90.7% less time)** without an object index.
An exclusively held object keeps its existing keys and unchanged values.
A shared object detaches before mutation; RHS reads finish before any write.
The one-field update needs one scan. General merges preserve original field
positions, first-match overrides and duplicate new RHS keys. The aliasing
fixture and 256 generated Rust cases compare order and duplicate behavior
with immutable addition. Callgrind instructions fell from **7,689,565,296 to
621,706,102 (91.9%)**. The list step's tree gain is preserved; other small
differences are not claimed as reliable speedups.

Native module ABI 2 and the C runtime are unchanged; Aquarium release wasm
remains byte-identical at 901,831 bytes.
Raw samples and metadata: [interpreter-object-results.json](interpreter-object-results.json).

Both container steps passed full workspace tests, clippy with warnings denied,
formatting, the wasm32 build, all 144 playground examples with the fresh wasm
interpreter, all 70 app fixtures in three modes, and both PR CI jobs. A separate
non-target spectral_norm check after both steps (three serial shuffled launches
per mode) gave **21.32 → 21.08 s**, essentially unchanged. Its raw samples are in
[interpreter-container-spectral-results.json](interpreter-container-spectral-results.json).
List [PR #50](https://github.com/codelovesme/code/pull/50) and object
[PR #51](https://github.com/codelovesme/code/pull/51) are merged; no release was made.


## Interpreter list growth (2026-10-16, unreleased)

Measured independently after text growth, against main `f7cc234`. Release-built
CLIs run each source with explicit `--interpret`: 15 serial fresh-process launches
per mode, deterministic shuffled before/after order, logical CPU 0.

| Workload | Before | After | Elapsed-time change |
|---|---:|---:|---:|
| binary_trees | 1382.90 ms | 102.47 ms | -92.6% |
| dictionary | 755.75 ms | 750.35 ms | -0.7% |
| string_build | 13.40 ms | 13.55 ms | +1.1% |
| points | 261.75 ms | 263.51 ms | +0.7% |
| sum_squares | 403.16 ms | 402.80 ms | -0.1% |

**Binary_trees is 13.5× faster (92.6% less time)**. List self-add assignments
grow the exclusively held vector instead of cloning its existing elements
on every append. `Rc::make_mut` detaches shared lists; evaluation finishes
before the write, so self/contained aliases, handler replies, iteration
snapshots and RHS errors retain their existing behavior. Array concatenation
still flattens one level, and a non-array operand remains one element.
Callgrind instructions fell from **11,434,478,715 to 803,451,119 (93.0%)**.
Other workloads show only small differences. Object merging is measured separately above.

Raw samples and metadata: [interpreter-list-results.json](interpreter-list-results.json).

## Interpreter text growth (2026-10-15, unreleased)

`code run --interpret` measured with release-built CLIs, main `d7f9614`
against implementation `acefcd5`. These are interpreter measurements, separate
from the prebuilt native executable table below. Fresh process launches include
parsing and bytecode lowering; before/after order was shuffled deterministically
and pinned to logical CPU 0. Each result is a median of 15 launches, except
the non-target spectral_norm check (three launches per mode).

| Workload | Before | After | Elapsed-time change |
|---|---:|---:|---:|
| string_build | 330.28 ms | 16.35 ms | -95.1% |
| dictionary | 789.79 ms | 759.59 ms | -3.8% |
| binary_trees | 1743.18 ms | 1414.69 ms | -18.8% |
| points | 273.60 ms | 260.95 ms | -4.6% |
| sum_squares | 417.27 ms | 414.76 ms | -0.6% |
| spectral_norm | 24803.14 ms | 25018.44 ms | +0.9% |

String_build is **20.2× faster (95.1% less elapsed time)**. Self-add text
assignments retain capacity and grow uniquely held strings instead of copying
the accumulated prefix on every addition. Shared strings detach before a
write; both operands finish evaluating before mutation, so self-append,
snapshots and errors preserve their existing behavior. Callgrind instructions
fell from 2,395,382,234 to 63,476,070 (97.4%). Ordinary additions
of other types retain their existing semantics. Numerical-loop performance
is essentially unchanged; this does not close its JavaScript/V8 gap.

The initial trial exposed an unlowered-expression panic. The repaired path
reads the resolved binding directly and lowers only the executable right
operand. Full workspace tests, clippy, formatting, wasm32 build, 144 playground
examples against the fresh wasm engine and all 70 app fixtures in three modes
passed. Aquarium release wasm remains byte-identical at 901,831 bytes.
Native module ABI 2 and the C runtime are unchanged. Rust embedding
callers construct text as `Value::Str(text.into())`.

Raw samples and measurement metadata: [interpreter-text-results.json](interpreter-text-results.json).

## Current measurement (2026-10-15, ticket 126 step 2-retry, median of 15)

| Workload | C | Rust | code | code / C |
|---|---|---|---|---|
| spectral_norm | 0.15746 s | 0.15721 s | 0.21421 s | 1.36× |
| binary_trees | 0.00341 s | 0.00368 s | 0.01099 s | 3.22× |
| string_build | 0.00149 s | 0.00125 s | 0.00166 s | 1.12× |
| dictionary | 0.00363 s | 0.00302 s | 0.00656 s | 1.80× |
| points | 0.00078 s | 0.00104 s | 0.00240 s | 3.06× |

Pinned to logical CPU 0; fresh prebuilt launches include startup.
These launches put code at 1.12–3.22× C's time. Ticket 126's large-object
index cut dictionary from 17.8 to 10.4 ms. Inline container lengths and empty-list
checks, literal-key templates and transferring a last stack value cut binary_trees
from 13.8 to 11.0 ms (129.6M → 101.8M instructions). The small-block pool was
rejected for insufficient elapsed-time gain. Interpolation now appends directly into uniquely held text, with correct active-frame
allocation of its pointer buffer: string_build fell from 4.6 to 1.7 ms and
dictionary from 10.3 to 6.6 ms.
The older tables below record development stages.

## Baseline (2026-10-11, code 2.14.8 + 113, i7-5930K, median of 5)

| Workload | C | Rust | code | code / C |
|---|---|---|---|---|
| spectral_norm | 0.167 s | 0.165 s | 0.945 s | 6× |
| binary_trees | 0.004 s | 0.005 s | 1.940 s | 500× |
| string_build | 0.0015 s | 0.0012 s | 0.111 s | 76× |
| dictionary | 0.0035 s | 0.0029 s | 0.276 s | 79× |
| points | 0.0007 s | 0.0010 s | 0.996 s | 1,370× |

C and Rust finish the last four in a few milliseconds, mostly process
start-up, so those ratios are lower bounds.

Where `code`'s time goes (callgrind): `binary_trees` and `points` spend ~90%
in `code_add` and `code_release` — `xs += x` copies the whole list and
releases the old one, so building n elements is n² work. `string_build` is
~95% `memcpy`/`strlen` inside string `+` for the same reason, `dictionary`
~85% in object merge (`obj + { k = v }` copies every key and value).
`spectral_norm` is the boxed path: `code_index` copying the element 18%,
`code_mul` 16%, `code_number` 14%, `code_copy` 10%.

## After in-place growth (2026-10-11, ticket 121)

`name += value` / `name = name + value` grows the variable's own list, text
or object where it stands when nothing else holds it.

| Workload | C | Rust | code | code / C | was |
|---|---|---|---|---|---|
| spectral_norm | 0.165 s | 0.167 s | 0.823 s | 5× | 6× |
| binary_trees | 0.0034 s | 0.0036 s | 0.027 s | 8× | 500× |
| string_build | 0.0015 s | 0.0012 s | 0.017 s | 12× | 76× |
| dictionary | 0.0035 s | 0.0029 s | 0.083 s | 24× | 79× |
| points | 0.0007 s | 0.0010 s | 0.054 s | 77× | 1,370× |

What is left (callgrind): `string_build` ~70% measuring the text's length
again on every append (`strlen`); `dictionary` ~64% and `points` ~26%
finding a field by comparing key text (`strcmp` in `find_field` /
`code_field`); `points` also building and copying its objects; the rest of
`binary_trees` is spread thin over release, copy and building.

## Setting a part (2026-10-11)

`pts[i].x = x` now changes one point where it stands, so `points.code` does
what the C and Rust versions do instead of building the next list every step
(the old program is still a valid one, and still runs at its old speed):

| Workload | C | code | code / C | was |
|---|---|---|---|---|
| points | 0.0009 s | 0.042 s | 50× | 77× (rebuilt every step) |

What is left there is reading, not writing (callgrind): `strcmp` ~17%,
`code_copy` 15%, `code_field` 10%, `code_index` 8% — every `pts[i].x` copies
the point out and finds `x` by comparing key text.

## Reading without copying (2026-10-11, ticket 120)

A field or element read that is only looked at points at the value instead of
copying it out, and `-`, `*`, `/` and ordering take a boxed operand's number
with one call and compute in line.

| Workload | C | code | code / C | was |
|---|---|---|---|---|
| spectral_norm | 0.167 s | 0.493 s | 3× | 5× |
| points | 0.0007 s | 0.034 s | 48× | 50× |
| binary_trees | 0.0034 s | 0.027 s | 8× | 8× |
| string_build | 0.0014 s | 0.017 s | 12× | 12× |
| dictionary | 0.0035 s | 0.075 s | 22× | 22× |

## Finding a field by hash (2026-10-11)

Every key is stored with its hash, and every `.field` read and part write
remembers where it found the field last. Instructions executed (callgrind),
before → after:

| Workload | before | after | code / C (time) |
|---|---|---|---|
| dictionary | 674M | 204M | 22× → 5× |
| points | 339M | 239M | 48× → ~40× |
| spectral_norm | 5,246M | 4,926M | 3× |
| binary_trees | 232M | 234M | 8× |
| string_build | 120M | 120M | 12× |

## Text length kept (2026-10-11)

A text's byte length is kept in its block header, so appending no longer
measures the whole text again: string_build 120M → 39M instructions,
0.017 s → 0.0046 s, **12× → 3× C**.

## Slicing in place, null checks in line (2026-10-11)

`stack = stack[0, length - 1)` cuts the list where it stands when nothing
else holds it, and `x = null` is a kind check rather than a call:
binary_trees 234M → ~165M instructions, **9× → 6× C**.

| Workload | C | code | code / C |
|---|---|---|---|
| spectral_norm | 0.167 s | 0.475 s | 3× |
| binary_trees | 0.0033 s | 0.021 s | 6× |
| string_build | 0.0014 s | 0.005 s | 3–4× |
| dictionary | 0.0038 s | 0.021 s | 6× |
| points | 0.0007 s | 0.028 s | 40× |

## Shapes, in-line reads, literal constants (2026-10-11, ticket 122 stage B)

Objects built from one literal share a shape; a `.field` read or part write
that knows it reads the slot directly; `xs[n]` is found in line; string
literals are constants.

| Workload | C | code | code / C |
|---|---|---|---|
| spectral_norm | 0.161 s | 0.420 s | 3× |
| binary_trees | 0.0034 s | 0.019–0.021 s | 6× |
| string_build | 0.0015 s | 0.005 s | 3–4× |
| dictionary | 0.0034 s | 0.020 s | 6× |
| points | 0.0007 s | 0.015 s | ~20× |

## Numbers in line where the program only makes numbers (2026-10-11)

Reads and variables the inference vouches are numbers are checked and read in
line; a number written along a known path is stored in line.

| Workload | C | code | code / C |
|---|---|---|---|
| spectral_norm | 0.167 s | 0.275 s | 1.6× |
| binary_trees | 0.0034 s | 0.020 s | 6× |
| string_build | 0.0014 s | 0.0046 s | 3× |
| dictionary | 0.0035 s | 0.018 s | 5× |
| points | 0.0008 s | 0.0084 s | ~10× |

## A fast version of each loop, and the optimizer on `--release` (2026-10-11)

A loop with no loop inside it compiles twice. In the fast version a check
that fails leaves for the same statement in a version that only calls the
runtime, instead of a slow path joining back in. `--release` now runs LLVM's
-O2 pipeline, after telling it which memory each access touches. `pts[i]`
is now checked once per iteration rather than at every read: points goes
from 60M to 17.7M instructions.

| Workload | C | code | code / C |
|---|---|---|---|
| spectral_norm | 0.166 s | 0.224 s | 1.35× |
| binary_trees | 0.0039 s | 0.020 s | 5× |
| string_build | 0.0014 s | 0.0046 s | 3× |
| dictionary | 0.0034 s | 0.0185 s | 5× |
| points | 0.0007 s | 0.0036 s | ~5× |

The rest of `points`' gap is memory. A point takes about 500 bytes of
80-byte value slots, where C's takes 32 bytes.

## 16-byte values (2026-10-12)

A value is now its kind, a flag and one 8-byte payload, 16 bytes in all. It
used to be 80, with a field for every kind side by side. A list's count and
an object's keys moved into the list's block. Points no longer spills out of
the cache.

| Workload | C | code | code / C |
|---|---|---|---|
| spectral_norm | 0.167 s | 0.223 s | 1.3× |
| binary_trees | 0.0036 s | 0.014 s | 4× |
| string_build | 0.0014 s | 0.0045 s | 3× |
| dictionary | 0.0047 s | 0.021 s | 4–6× |
| points | 0.0010 s | 0.0029 s | ~3× |

Side by side with the 80-byte build, as the same binaries: points 3.86 → 2.66
ms and binary_trees 20.3 → 15.7 ms. dictionary is unchanged; its time is the
key search.

Length migration (unreleased, ticket 130): string_build now validates its
108890-character result by the last in-range and first out-of-range indices.
The build loop and expected length are unchanged; this keeps the runtime
workload independent of separately installed utility modules after core.Length
removal. Historical raw timings used the earlier core Length assertion and
are not silently reclassified as measurements of the new fixture.


## Length module boundary (ticket 131, source only)

`python3 benchmarks/module_length.py BEFORE AFTER --out OUTPUT` reproduces
the frozen-artifact experiment. Each directory contains the CLI, native arrays/
strings libraries and wasm archives. The committed module-length-results.json
contains all hashes, source hashes and raw samples. CPU 0, seed 131, serial
shuffled fresh launches, nine samples; wasm uses nine warm handler samples
after five warmups. No concurrent local tests/builds/profiling during timing.
Both CLIs use Rust 1.97.1, Clang 17.0.6 and GCC 14.2.0. The baseline is
main 116cf88, already using module Length, rather than an older core handler.

Each row performs 2,000 calls with assertions. Interpreter/native process
totals include startup, parsing and container construction; warm wasm timings
only cover the handler. Zero-call controls are in the raw data. Medians, ms:

| Input | Interpreter before → after | Compiled wasm before → after |
|---|---:|---:|
| Array, 10 numbers | 9.539 → 8.237 | 7.798 → 1.289 |
| Array, 1,000 numbers | 28.699 → 8.396 | 95.753 → 1.156 |
| Array, 10,000 numbers | 213.965 → 10.905 | 892.754 → 1.063 |
| Array, 1,000 nested records | 748.737 → 8.648 | 2633.301 → 1.231 |
| Unicode text, 30,000 scalars | 133.360 → 25.766 | 2329.370 → 120.032 |

The large numeric array's zero-call-adjusted interpreter cost is 204.840 →
1.529 ms. Its element count no longer changes the cost of the call path.
Unicode counting still scans text. Native compiled Length already borrows ABI
values; its numeric-array process total is 2.430 → 2.400 ms, not an additional
speed win. Small native totals vary: nested records 1.742 → 1.891 ms.
Fallback handlers remain measured: wasm Trim 6.206 → 6.499 ms (+4.7%, 0.293 ms
per 2,000 calls); Reverse 8.878 → 8.945 ms (+0.8%). Interpreter Trim/Reverse
use 9.643 → 8.826 and 10.384 → 9.628 ms. These costs are retained visibly.
No claim is made about general language speed, application FPS, or the separate
code-wasm JSON callback interpreter, whose interface remains unchanged.

Callgrind on the original 10,000-literal/2,000-call probe: 1,283,658,818 →
50,603,899 instructions (96.06% fewer). Arena::build/list accounted for 70.15%
of baseline self instructions. Profiling an initial string prototype identified
redundant UTF-8 validation (82.75% of instructions); the optional view now
requires valid UTF-8 explicitly and borrows it without that second validation.
All metadata pointers remain synchronous, read-only, thread-confined borrows.
Local full inputs, frozen binaries, profiles and logs: /var/tmp/length-131/.

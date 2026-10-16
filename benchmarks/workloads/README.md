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

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

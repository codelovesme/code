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

The language has no recursion, no element assignment, no `%`, `floor` or
`sqrt`, so C and Rust use the same iterative steps; where `code` can only
rebuild a list or an object, C and Rust use their idiomatic in-place form —
that difference is part of what is measured. Classic workloads that need
element writes (sieve, fannkuch) or `sqrt` (n-body) are left out until the
language can express them.

```sh
CODE_BIN=/path/to/code benchmarks/workloads/run.py [--runs 5] [workload ...]
```

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

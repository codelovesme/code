# Compile-time baseline

Where `code build` spends its time, stage by stage (`code build --timings`,
ticket 115). Each compile-speed change adds a section below with the same
three programs, so a gain — or a regression — is a number, not a feeling.

    benchmarks/compile-time/run.py [--code PATH] [--apps PATH] [--runs N]

Machine: Intel Core i7-5930K, 12 logical cores, Debian 13.

## 2026-10-10 — before any compile-speed work

`Code v2.14.5`, median of 5 builds, ms

**sum_squares (14 lines), exe**

| stage | ms |
|---|---|
| load and parse | 0.1 |
| checks | 0.0 |
| IR generation | 0.3 |
| IR verify | 0.1 |
| LLVM backend | 11.2 |
| runtime compile and link | 263.8 |
| other | 0.6 |
| total | 276.4 |

**todo-api, shared**

| stage | ms |
|---|---|
| load and parse | 89.3 |
| checks | 0.3 |
| IR generation | 18.5 |
| IR verify | 10.1 |
| LLVM backend | 811.6 |
| runtime compile and link | 264.1 |
| other | 6.3 |
| total | 1204.5 |

**aquarium-web, wasm**

| stage | ms |
|---|---|
| load and parse | 130.6 |
| checks | 0.9 |
| IR generation | 54.8 |
| IR verify | 45.1 |
| LLVM backend | 3290.5 |
| runtime compile | 851.7 |
| link | 124.3 |
| other | 35.7 |
| total | 4525.8 |

Read: a small program is almost all the C runtime's compile (done on every
build, without optimisation, inside the `cc` link — ticket 111); a large one
is mostly LLVM's backend on one core (ticket 116) over a lot of IR
(ticket 117).

## 2026-10-10 — after 111 (runtime compiled once, embedded, -O2)

`Code v2.14.6`, median of 5 builds, ms

**sum_squares (14 lines), exe**

| stage | ms |
|---|---|
| load and parse | 0.1 |
| checks | 0.0 |
| IR generation | 0.2 |
| IR verify | 0.1 |
| LLVM backend | 9.2 |
| link | 22.3 |
| other | 0.5 |
| total | 34.1 |

**todo-api, shared**

| stage | ms |
|---|---|
| load and parse | 95.5 |
| checks | 0.2 |
| IR generation | 14.8 |
| IR verify | 8.8 |
| LLVM backend | 810.2 |
| link | 31.1 |
| other | 5.9 |
| total | 964.6 |

**aquarium-web, wasm**

| stage | ms |
|---|---|
| load and parse | 133.9 |
| checks | 0.9 |
| IR generation | 56.1 |
| IR verify | 44.7 |
| LLVM backend | 3280.6 |
| link | 123.0 |
| other | 35.5 |
| total | 3681.1 |

Builds: sum_squares 276 → 34 ms, todo-api 1205 → 965 ms, aquarium-web 4526 → 3681 ms.
And every program now runs on an optimised runtime: sum_squares 828 → 336 ms.
What is left is almost all LLVM's backend (116, 117).

## 2026-10-10 — after 116 (machine code on every core, same output on any core count)

`Code v2.14.7`, median of 5 builds, ms

**sum_squares (14 lines), exe**

| stage | ms |
|---|---|
| load and parse | 0.1 |
| checks | 0.0 |
| IR generation | 0.3 |
| IR verify | 0.1 |
| LLVM backend | 11.5 |
| link | 24.6 |
| other | 0.4 |
| total | 35.7 |

**todo-api, shared**

| stage | ms |
|---|---|
| load and parse | 98.1 |
| checks | 0.3 |
| IR generation | 16.7 |
| IR verify | 8.6 |
| LLVM backend | 238.4 |
| link | 23.4 |
| other | 9.6 |
| total | 388.7 |

**aquarium-web, wasm**

| stage | ms |
|---|---|
| load and parse | 128.8 |
| checks | 1.0 |
| IR generation | 54.6 |
| IR verify | 44.2 |
| LLVM backend | 955.3 |
| link | 112.4 |
| other | 41.7 |
| total | 1335.1 |

Builds: todo-api 965 → 389 ms, aquarium-web 3681 → 1335 ms; sum_squares unchanged
(one part). On one core Aquarium's backend is 3.45 s against 3.29 s unsplit.

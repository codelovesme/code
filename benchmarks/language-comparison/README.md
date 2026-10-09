# `code` / C / Java loop benchmark

This benchmark compares one numeric workload in the `code` interpreter, the
`code` LLVM native backend, C, and Java. It calculates the sum of squares from
0 through 99,999 one hundred times, checking the result after each repetition.
All four programs use double-precision numbers and the same loop bounds and
arithmetic. The workload performs 10,000,000 inner-loop iterations per process.

It is intentionally a small, transparent data point. It does not model object
allocation, I/O, startup-sensitive tiny scripts, concurrency, or every style of
program, and it cannot establish a universal language ranking.

## Run it

Requires Python 3, a `code` executable built from this checkout, GCC, and a Java
26 JDK. The harness compiles the C, Java, and native `code` programs first,
then times seven fresh process launches per mode. Compilation is timed
separately over five compiler invocations. Each run's wall time includes
process startup. On Linux, the harness pins itself and its child processes to
the lowest logical CPU allowed by the current process; pass `--no-pin` to
disable this.

```sh
CODE_BIN=/path/to/code benchmarks/language-comparison/run.py
```

The script writes `results.csv` with all timed samples and `results.json` with
the summary, toolchain versions, and compiler times. Override sample counts
with `--runs` and `--compile-runs`; override compiler paths with `CC`, `JAVA`,
and `JAVAC`.

## Limits

The timing includes each program's process startup and, for `code run`, parsing
and interpretation. The Java process starts cold on every sample, then the
workload gives HotSpot a chance to compile hot loops. Compiler output is not
included in runtime measurements. The C binaries are built with
`-O2 -march=native` and `-O3 -march=native`; the `code` native binary uses the
CLI's LLVM `--release` optimization level (`-O2`); Java uses `javac --release
26` and default HotSpot settings. These are optimized but different
implementations and runtimes.

No peak-RSS or allocation-rate measurement is included. This benchmark tests
numeric execution time only; memory management differences in the comparison
page are drawn from the languages' documented runtime designs.

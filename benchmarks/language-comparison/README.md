# `code` / C / Java / Python / Rust loop benchmark

This benchmark compares one numeric workload in the `code` interpreter, the
`code` LLVM native backend, C, Java, Python, and Rust. It calculates the sum of
squares from 0 through 99,999 one hundred times, checking the result after each
repetition. All six programs use double-precision numbers and the same loop
bounds and arithmetic. The workload performs 10,000,000 inner-loop iterations
per process.

It is intentionally a small, transparent data point. It does not model object
allocation, I/O, startup-sensitive tiny scripts, concurrency, or every style of
program, and it cannot establish a universal language ranking.

## Run it

Requires Python 3.10 or newer, a `code` executable built from this checkout,
GCC, a Java 26 JDK, and `rustc`. The harness compiles the C, Java, Rust, and
native `code` programs first, then times seven fresh process launches per mode.
Compilation is timed separately over five compiler invocations. Each run's wall time
includes process startup. On Linux, the harness pins itself and its child
processes to the lowest logical CPU allowed by the current process; pass
`--no-pin` to disable this. Rust is compiled directly with `rustc`, using
edition 2024, optimization level 3, and the local CPU target; Cargo is not part
of the measured compiler launch.

```sh
CODE_BIN=/path/to/code benchmarks/language-comparison/run.py
```

The script writes `results.csv` with all timed samples and `results.json` with
the summary, toolchain versions, and compiler times. Override sample counts
with `--runs` and `--compile-runs`; override compiler paths with `CC`, `JAVA`,
`JAVAC`, `PYTHON`, and `RUSTC`. If the `code` executable was built from a
different revision than the checkout, set `CODE_COMMIT` to the executable's
source revision so the report records the measured compiler accurately.

## Limits

The timing includes each program's process startup and, for `code run` and
Python, source processing and execution. Each `python3 sum_squares.py` launch
passes the `.py` source as the program; CPython parses and compiles it to
bytecode during the timed run. No precompiled `.pyc` file is passed as the
benchmark program, and no separate Python build time is reported. Python
uses `float` values, which are IEEE 754 binary64, to match the other
implementations' number type. The Java process starts cold on every sample,
then the workload gives HotSpot a chance to compile hot loops. Separate
compiler/build-process times are not added to runtime measurements; HotSpot's
JIT work is included in Java's run time. The C binaries are built with
`-O2 -march=native` and `-O3 -march=native`; Rust uses `rustc -C opt-level=3
-C target-cpu=native`; the `code` native binary uses the CLI's LLVM `--release`
optimization level (`-O2`); Java uses `javac --release 26` and default HotSpot
settings. These are optimized but different
implementations and runtimes.

No peak-RSS or allocation-rate measurement is included. This benchmark tests
numeric execution time only; memory management differences in the comparison
page are drawn from the languages' documented runtime designs.

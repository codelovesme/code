# `code` / C / Java / Python / JavaScript / Rust / Go benchmark

This benchmark compares one numeric workload in two separate runtime groups:

- **Prebuilt programs:** `code build --release`, C, Java, Rust, and Go. Their
  artifacts are built before timing and then run as fresh processes.
- **Source-run programs:** `code run`, Python, and JavaScript under Node.js/V8.
  Source processing is part of each measured launch. CPython generates
  bytecode while starting; Node/V8 parses JavaScript and may JIT-compile hot
  code while the program runs.

All implementations calculate the sum of squares from 0 through 99,999 one
hundred times, checking the result after each repetition. They use IEEE 754
double-precision values and execute 10,000,000 inner-loop iterations per
process. JavaScript runs on Node.js, which uses the V8 engine; this is a Node.js
measurement, not a browser-engine measurement.

This is a small, transparent data point. It does not model object allocation,
I/O, startup-sensitive tiny scripts, concurrency, or every style of program,
and it cannot establish a universal language ranking. It does not measure
memory use.

## Run it

Requires Python 3.10 or newer, a `code` executable built from this checkout,
GCC, a Java 26 JDK, Node.js, `rustc`, and the Go toolchain. The harness compiles
the C, Java, Rust, Go, and native `code` programs first, then times seven fresh
process launches per mode. Compilation is timed separately over five compiler
invocations. Each runtime measurement includes process startup. On Linux, the
harness pins itself and its child processes to the lowest logical CPU allowed
by the current process; pass `--no-pin` to disable this. Rust is compiled
directly with `rustc`, using edition 2024, optimization level 3, and the local
CPU target; Cargo is not part of the measured compiler launch. Go is compiled
with `GO111MODULE=off go build -trimpath` and the toolchain's default
optimization settings. The standalone-file setting avoids needing a module
for this dependency-free benchmark.

An untimed Go build warms the cache for this program's standard-library
dependencies. Each timed Go build varies only a source comment so the benchmark
package is rebuilt while those dependencies can remain cached.

```sh
CODE_BIN=/path/to/code benchmarks/language-comparison/run.py
```

The script writes `results.csv` with all timed runtime samples and
`results.json` with the summary, toolchain versions, and compiler times.
Override sample counts with `--runs` and `--compile-runs`; override compiler
and runtime paths with `CC`, `JAVA`, `JAVAC`, `PYTHON`, `NODE`, `RUSTC`, and
`GO`. If the `code` executable was built from a different revision than the
checkout, set `CODE_COMMIT` to the executable's source revision so the report
records the measured compiler accurately.

## Limits and timing boundaries

`code run`, Python, and JavaScript pass their source files directly to their
runtimes. There is no separate build invocation for these three modes, so they
do not appear in the ahead-of-time build-time table. For Python, source parsing
and CPython bytecode generation happen during the timed launch; no precompiled
`.pyc` file is run. For JavaScript, Node/V8 startup and source parsing, plus any
JIT compilation during execution, are included in the timed launch; no
precompiled JavaScript artifact is run. V8 can tier and optimize code as it
runs, so the number here is specific to this short-lived process and workload.

The other modes run artifacts built before timing. Java's `.class` bytecode
is produced separately, while JVM startup and HotSpot JIT work remain in Java's
runtime measurement. C binaries use `-O2 -march=native` and
`-O3 -march=native`; Rust uses `rustc -C opt-level=3 -C target-cpu=native`; the
`code` native binary uses the CLI's LLVM `--release` optimization level
(`-O2`); Java uses `javac --release 26` and default HotSpot settings; Go uses
`go build` with the Go compiler's default optimization settings. These are
optimized but different implementations and runtimes.

No peak-RSS or allocation-rate measurement is included. The benchmark tests
numeric execution time only; memory-management differences in the comparison
page are based on the languages' documented runtime designs.

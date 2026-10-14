# `code` / C / Java / Python / JavaScript / Rust / Go benchmark

This benchmark compares one numeric workload in two separate runtime groups:

- **Prebuilt programs:** `code build --release`, C, Java, Rust, and Go. Their
  artifacts are built before timing and then run as fresh processes.
- **Source-run programs:** `code run` with cold and warm build caches,
  `code run --interpret`, Python, and JavaScript under Node.js/V8.
  Preparation needed for each mode is part of its measured launch. CPython
  generates bytecode while starting; Node/V8 parses JavaScript and may JIT-compile hot
  code while the program runs.

All implementations calculate the sum of squares from 0 through 99,999 one
hundred times, checking the result after each repetition. They use IEEE 754
double-precision values and execute 10,000,000 inner-loop iterations per
process. JavaScript runs on Node.js, which uses the V8 engine; this is a Node.js
measurement, not a browser-engine measurement.

This is a small, transparent data point. It does not model object allocation,
I/O, startup-sensitive tiny scripts, concurrency, or every style of program,
and it cannot establish a universal language ranking. It also records peak
resident memory for each launch, which describes this loop's process footprint
rather than its allocation rate or a whole application's memory needs.

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

The script writes `results.csv` with every launch's elapsed time and peak RSS,
plus the median and full RSS range per mode. `results.json` records those
samples and summaries, toolchain versions, and compiler times. Peak RSS
measurement requires Linux.
Override sample counts with `--runs` and `--compile-runs`; override compiler
and runtime paths with `CC`, `JAVA`, `JAVAC`, `PYTHON`, `NODE`, `RUSTC`, and
`GO`. If the `code` executable was built from a different revision than the
checkout, set `CODE_COMMIT` to the executable's source revision so the report
records the measured compiler accurately.

## Limits and timing boundaries

`code run` now builds and caches a native executable, then runs it. The harness
measures three source-run modes:

- **`code run` (cold cache):** each launch gets an empty `CODE_CACHE_DIR`.
  Compilation, cache writes, and native execution are inside the timer;
  creating the directory is outside it. This is a cold build cache, not a
  cold OS file cache.
- **`code run` (warm cache):** one untimed launch populates a separate cache.
  All timed launches reuse it, including cache validation and native execution.
- **`code run --interpret`:** parsing and lowering to register bytecode are
  included. Typed number operations use plain numeric registers and slots.

The harness unsets `CODE_RUN` and sets `CODE_CACHE=1` for these modes, so caller
overrides cannot silently turn the native modes into interpreter runs or disable
the cache. It checks that the warm-up populated the native run cache. The small
benchmark is built at LLVM `-O2` on its first launch; larger programs can start
with `-O0` while an optimized build runs in the background.

These modes, Python, and JavaScript pass source paths to their runtimes. They
have no separately timed build invocation; the cold-cache `code run` build is
included in its launch time. For Python, source parsing
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

`peak.c`, built with the same C compiler as the benchmark, does
fork/exec/wait4 and records Linux `ru_maxrss` in KiB. Launching through this
small C process avoids inheriting Python's roughly 10 MB resident high-water
mark. Its monotonic timer surrounds fork/exec/wait, excluding the wrapper's
own startup. Child output and failing exit statuses are preserved.

RSS is resident physical memory, not heap allocation. For a mode with child
processes, it records the largest process peak, including waited descendants;
it does not add their simultaneously resident memory. In particular,
`code run` includes the LLVM-linked driver that loads and keys the source,
whereas the prebuilt native mode runs only the small built program. A cold
cache launch also includes compiler descendants.

The numeric loop is code's best case. See `../workloads/` for list, record,
text, and allocation workloads, with their own recorded samples.

# Numeric comparison rerun — 2026-10-16

Source revision `3171efb`, release-built workspace CLI through `code-launcher`.
The existing benchmark was rerun unchanged: 10 million double-precision inner
iterations, each repetition checks its answer, 15 fresh launches per mode,
five compiler launches per compiler. Deterministic shuffled order; logical CPU 0;
no concurrent agent builds, tests or profiling during the measurement.

| Mode | Median | Range | Median peak RSS |
|---|---:|---:|---:|
| code build --release (native) | 8.89 ms | 8.83–9.21 ms | 1.20 MiB |
| Rust (rustc -C opt-level=3) | 9.05 ms | 9.00–9.21 ms | 1.96 MiB |
| code run (warm cache) | 9.54 ms | 9.49–9.66 ms | 2.08 MiB |
| C (GCC -O3 -march=native) | 14.45 ms | 14.40–14.86 ms | 1.18 MiB |
| C (GCC -O2 -march=native) | 14.45 ms | 14.37–14.68 ms | 1.19 MiB |
| Go (go build; default optimizations) | 15.19 ms | 15.15–15.42 ms | 1.81 MiB |
| JavaScript (Node.js/V8) | 42.60 ms | 39.75–48.11 ms | 48.02 MiB |
| code run (cold cache) | 44.42 ms | 43.87–45.72 ms | 39.53 MiB |
| Java 26 (HotSpot default) | 53.30 ms | 52.59–55.32 ms | 34.00 MiB |
| code run --interpret (interpreter) | 404.03 ms | 398.27–435.32 ms | 29.50 MiB |
| Python (CPython) | 1381.19 ms | 1291.69–1469.04 ms | 8.39 MiB |

For this numeric workload, interpretation is **9.49× slower than Node/V8**
and **3.42× faster than CPython**. It remains around the previous 403 ms.
The recent list/object gains are for different workloads; this is one scalar
numeric loop and cannot establish a universal language ranking.

Every runtime includes process startup. Native/C/Rust/Go artifacts and Java
bytecode are built before timing; their build times are recorded separately.
Interpreter parsing/bytecode lowering, Python bytecode generation and Java/JS
runtime JIT work remain inside their launch measurements. RSS is the maximum
resident process including waited descendants, rather than a sum of tree memory.

Reproduce using the existing harness and the recorded toolchain paths:

```sh
CODE_BIN="$PWD/target/release/code-launcher" \
JAVA=/tmp/jdk-26.0.2.1+1/bin/java \
JAVAC=/tmp/jdk-26.0.2.1+1/bin/javac \
GO=/tmp/go1.27.1/bin/go CODE_COMMIT=3171efb \
python3 benchmarks/language-comparison/run.py --runs 15 --compile-runs 5 \
  --output-dir benchmarks/language-comparison/reruns/2026-10-16
```

[Full report and toolchains](results.json) · [Raw launches](results.csv)

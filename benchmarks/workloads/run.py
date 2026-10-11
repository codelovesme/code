#!/usr/bin/env python3
"""Workloads beyond one numeric loop: the same algorithm in code, C and Rust.

Each program checks its own answer and exits non-zero if it is wrong. Every
binary is built first, then run `--runs` times as a fresh process; the table
is the median wall time. C is `gcc -O2 -march=native`, Rust `rustc -C
opt-level=3 -C target-cpu=native`, code `code build --release`.

    CODE_BIN=/path/to/code benchmarks/workloads/run.py [--runs 5] [only ...]
"""
import argparse, os, shutil, statistics, subprocess, sys, tempfile, time
from pathlib import Path

HERE = Path(__file__).resolve().parent
WORKLOADS = ["spectral_norm", "binary_trees", "string_build", "dictionary", "points"]

def run(cmd, **kw):
    r = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if r.returncode:
        sys.exit(f"failed ({r.returncode}): {' '.join(map(str, cmd))}\n{r.stdout}{r.stderr}")

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("only", nargs="*")
    args = ap.parse_args()
    code = os.environ.get("CODE_BIN", "code")
    out = Path(tempfile.mkdtemp(prefix="code-workloads-"))
    env = dict(os.environ, CODE_CACHE="0")
    rows = []
    for w in args.only or WORKLOADS:
        bins = {}
        run(["gcc", "-std=c17", "-O2", "-march=native", HERE / f"{w}.c", "-o", out / f"{w}-c", "-lm"])
        bins["C"] = out / f"{w}-c"
        run(["rustc", "--edition", "2024", "-C", "opt-level=3", "-C", "target-cpu=native", HERE / f"{w}.rs", "-o", out / f"{w}-rs"])
        bins["Rust"] = out / f"{w}-rs"
        run([code, "build", HERE / f"{w}.code", "--release", "-o", out / f"{w}-code"], env=env)
        bins["code"] = out / f"{w}-code"
        times = {}
        for name, b in bins.items():
            samples = []
            for _ in range(args.runs):
                t = time.perf_counter()
                run([b])
                samples.append(time.perf_counter() - t)
            times[name] = statistics.median(samples)
        rows.append((w, times))
        print(f"{w:15} C {times['C']:8.4f} s   Rust {times['Rust']:8.4f} s   code {times['code']:8.4f} s"
              f"   code/C {times['code'] / times['C']:7.0f}x", flush=True)
    shutil.rmtree(out)

if __name__ == "__main__":
    main()

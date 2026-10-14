#!/usr/bin/env python3
"""Workloads beyond one numeric loop: the same algorithm in code, C and Rust.

Each program checks its own answer and exits non-zero if it is wrong. Every
binary is built first, then run `--runs` times as a fresh process; the table
is the median wall time. C is `gcc -O2 -march=native`, Rust `rustc -C
opt-level=3 -C target-cpu=native`, code `code build --release`.

    CODE_BIN=/path/to/code benchmarks/workloads/run.py [--runs 5] [only ...]
"""
import argparse, csv, datetime, json, os, random, shutil, statistics, subprocess, sys, tempfile, time
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
    ap.add_argument("--output-dir", type=Path, default=HERE)
    ap.add_argument("--no-pin", action="store_true")
    ap.add_argument("only", nargs="*")
    args = ap.parse_args()
    if args.runs < 1:
        ap.error("--runs must be positive")
    if any(w not in WORKLOADS for w in args.only):
        ap.error("unknown workload")
    cpu = None
    if not args.no_pin and hasattr(os, "sched_getaffinity"):
        cpu = min(os.sched_getaffinity(0))
        os.sched_setaffinity(0, {cpu})
    code = os.environ.get("CODE_BIN", "code")
    out = Path(tempfile.mkdtemp(prefix="code-workloads-"))
    env = dict(os.environ, CODE_CACHE="0")
    rows = []
    raw = []
    rng = random.Random(20261014)
    for w in args.only or WORKLOADS:
        bins = {}
        run(["gcc", "-std=c17", "-O2", "-march=native", HERE / f"{w}.c", "-o", out / f"{w}-c", "-lm"])
        bins["C"] = out / f"{w}-c"
        run(["rustc", "--edition", "2024", "-C", "opt-level=3", "-C", "target-cpu=native", HERE / f"{w}.rs", "-o", out / f"{w}-rs"])
        bins["Rust"] = out / f"{w}-rs"
        run([code, "build", HERE / f"{w}.code", "--release", "-o", out / f"{w}-code"], env=env)
        bins["code"] = out / f"{w}-code"
        samples = {name: [] for name in bins}
        for index in range(args.runs):
            order = list(bins)
            rng.shuffle(order)
            for name in order:
                t = time.perf_counter()
                run([bins[name]])
                elapsed = time.perf_counter() - t
                samples[name].append(elapsed)
                raw.append({"workload": w, "mode": name, "run": index + 1, "elapsed_seconds": elapsed})
        times = {name: statistics.median(values) for name, values in samples.items()}
        rows.append({"workload": w, "samples_seconds": samples,
                     "median_seconds": times, "code_over_c": times["code"] / times["C"]})
        print(f"{w:15} C {times['C']:8.4f} s   Rust {times['Rust']:8.4f} s   code {times['code']:8.4f} s"
              f"   code/C {times['code'] / times['C']:7.2f}x", flush=True)
    shutil.rmtree(out)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    report = {"measured_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
              "code": subprocess.check_output([code, "--version"], text=True).strip(),
              "code_commit": os.environ.get("CODE_COMMIT") or subprocess.check_output(
                  ["git", "rev-parse", "--short", "HEAD"], cwd=HERE, text=True).strip(),
              "pinned_logical_cpu": cpu, "runs_per_mode": args.runs,
              "notes": ["Fresh prebuilt process launches, including startup; deterministic shuffled order per repetition.",
                        "code build --release; GCC -O2 -march=native; rustc -C opt-level=3 -C target-cpu=native.",
                        "Short workloads can be dominated by process startup; ratios describe these complete launches."],
              "summary": rows}
    (args.output_dir / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    with (args.output_dir / "results.csv").open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=["workload", "mode", "run", "elapsed_seconds"], lineterminator="\n")
        writer.writeheader()
        writer.writerows(raw)

if __name__ == "__main__":
    main()

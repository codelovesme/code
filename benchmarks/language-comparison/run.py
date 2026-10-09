#!/usr/bin/env python3
"""Run the small Code/C/Java/Python comparison benchmark on this machine."""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import json
import os
import platform
import random
import shlex
import statistics
import subprocess
import tempfile
import time
from pathlib import Path


HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
ITERATIONS = 100_000
REPETITIONS = 100
TOTAL_ITERATIONS = ITERATIONS * REPETITIONS


def command_from_env(name: str, default: str) -> list[str]:
    return shlex.split(os.environ.get(name, default))


def checked(command: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True)
    if result.returncode:
        raise RuntimeError(
            f"command failed ({result.returncode}): {' '.join(command)}\n"
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
    return result


def version(command: list[str], cwd: Path) -> str:
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True)
    return (result.stdout + result.stderr).strip().splitlines()[0]


def cpu_model() -> str:
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return "unknown"


def os_description() -> str:
    try:
        for line in Path("/etc/os-release").read_text().splitlines():
            if line.startswith("PRETTY_NAME="):
                return line.split("=", 1)[1].strip().strip('"')
    except OSError:
        pass
    return platform.platform()


def optional_version(command: list[str], cwd: Path) -> str | None:
    try:
        return version(command, cwd)
    except OSError:
        return None


def median(values: list[float]) -> float:
    return statistics.median(values)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runs", type=int, default=7, help="timed process launches per mode")
    parser.add_argument("--compile-runs", type=int, default=5, help="timed compiler launches")
    parser.add_argument("--no-pin", action="store_true", help="do not pin child processes to one CPU")
    parser.add_argument("--output-dir", type=Path, default=HERE, help="where to write CSV and JSON results")
    args = parser.parse_args()
    if args.runs < 1 or args.compile_runs < 1:
        parser.error("--runs and --compile-runs must be positive")

    code = command_from_env("CODE_BIN", "code")
    cc = command_from_env("CC", "gcc")
    javac = command_from_env("JAVAC", "javac")
    java = command_from_env("JAVA", "java")
    python = command_from_env("PYTHON", "python3")

    pinned_cpu = None
    if not args.no_pin and hasattr(os, "sched_getaffinity"):
        allowed = sorted(os.sched_getaffinity(0))
        if allowed:
            pinned_cpu = allowed[0]
            os.sched_setaffinity(0, {pinned_cpu})

    args.output_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="code-language-benchmark-") as temp_name:
        temp = Path(temp_name)
        code_out = temp / "code-native"
        c_out: dict[str, Path] = {}
        java_classes = temp / "java-classes"

        compile_builders = {
            "code build --release": lambda index: code + [
                "build", str(HERE / "sum_squares.code"), "--release", "-o",
                str(temp / f"code-native-{index}"),
            ],
            "javac --release 26": lambda index: javac + [
                "--release", "26", "-d", str(temp / f"java-classes-{index}"),
                str(HERE / "SumSquares.java"),
            ],
        }
        for optimization in ("-O2", "-O3"):
            label = f"GCC {optimization} -march=native"
            compile_builders[label] = lambda index, optimization=optimization: cc + [
                "-std=c17", optimization, "-march=native",
                str(HERE / "sum_squares.c"), "-o",
                str(temp / f"sum-squares-{optimization[1:]}-{index}"),
            ]
        compile_samples: dict[str, list[float]] = {}
        for label, build_command in compile_builders.items():
            compile_samples[label] = []
            for index in range(args.compile_runs):
                started = time.perf_counter()
                checked(build_command(index), REPO)
                compile_samples[label].append(time.perf_counter() - started)

        # Retain the last compiler output for each implementation.
        code_out = temp / f"code-native-{args.compile_runs - 1}"
        c_out = {
            optimization: temp / f"sum-squares-{optimization[1:]}-{args.compile_runs - 1}"
            for optimization in ("-O2", "-O3")
        }
        java_classes = temp / f"java-classes-{args.compile_runs - 1}"

        modes = {
            "code run (interpreter)": code + ["run", str(HERE / "sum_squares.code")],
            "code build --release (native)": [str(code_out)],
            "C (GCC -O2 -march=native)": [str(c_out["-O2"])],
            "C (GCC -O3 -march=native)": [str(c_out["-O3"])],
            "Java 26 (HotSpot default)": java + ["-cp", str(java_classes), "SumSquares"],
            "Python (CPython)": python + [str(HERE / "sum_squares.py")],
        }
        samples: dict[str, list[float]] = {name: [] for name in modes}
        order_rng = random.Random(20261009)
        for _ in range(args.runs):
            order = list(modes)
            order_rng.shuffle(order)
            for name in order:
                started = time.perf_counter()
                checked(modes[name], REPO)
                samples[name].append(time.perf_counter() - started)

        rows = []
        for name, values in samples.items():
            med = median(values)
            rows.append({
                "mode": name,
                "median_seconds": med,
                "min_seconds": min(values),
                "max_seconds": max(values),
                "million_iterations_per_second": TOTAL_ITERATIONS / med / 1_000_000,
            })

        with (args.output_dir / "results.csv").open("w", newline="") as handle:
            writer = csv.DictWriter(
                handle, fieldnames=["mode", "run", "elapsed_seconds"], lineterminator="\n"
            )
            writer.writeheader()
            for name, values in samples.items():
                for run_index, elapsed in enumerate(values, start=1):
                    writer.writerow({"mode": name, "run": run_index, "elapsed_seconds": f"{elapsed:.9f}"})

        report = {
            "measured_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"),
            "workload": {
                "inner_iterations_per_repetition": ITERATIONS,
                "repetitions": REPETITIONS,
                "total_inner_iterations_per_process": TOTAL_ITERATIONS,
                "operation": "double-precision sum of i*i for i=0..99,999, checked each repetition",
            },
            "environment": {
                "cpu": cpu_model(),
                "architecture": platform.machine(),
                "kernel": platform.release(),
                "distribution": os_description(),
                "pinned_logical_cpu": pinned_cpu,
                "code": version(code + ["--version"], REPO),
                "code_commit": os.environ.get("CODE_COMMIT") or checked(
                    ["git", "rev-parse", "--short", "HEAD"], REPO
                ).stdout.strip(),
                "llvm": optional_version(["llvm-config", "--version"], REPO),
                "c_compiler": version(cc + ["--version"], REPO),
                "java_runtime": version(java + ["-version"], REPO),
                "java_compiler": version(javac + ["-version"], REPO),
                "python_runtime": checked(
                    python + [
                        "-c",
                        "import platform; print(platform.python_implementation(), platform.python_version())",
                    ],
                    REPO,
                ).stdout.strip(),
                "code_compile_command": "code build --release (LLVM optimization level -O2)",
                "c_compile_commands": [
                    "gcc -std=c17 -O2 -march=native",
                    "gcc -std=c17 -O3 -march=native",
                ],
                "java_compile_command": "javac --release 26; HotSpot default runtime settings",
            },
            "timed_runs_per_mode": args.runs,
            "compile_runs_per_compiler": args.compile_runs,
            "compile_samples_seconds": compile_samples,
            "samples_seconds": samples,
            "summary": rows,
            "notes": [
                "Process startup is included in runtime measurements; compilation is excluded.",
                "Python and code run execute source directly; parsing is part of their runtime measurement.",
                "No separate memory measurement was made.",
                "One workload is a useful data point, not a universal language ranking.",
                "Python uses float (IEEE 754 binary64) values; its process startup and source parsing are included in run time.",
            ],
        }
        (args.output_dir / "results.json").write_text(json.dumps(report, indent=2) + "\n")

        print(f"CPU: {report['environment']['cpu']}")
        print(f"Pinned logical CPU: {pinned_cpu if pinned_cpu is not None else 'no'}")
        print(f"Workload: {TOTAL_ITERATIONS:,} inner iterations per process; {args.runs} timed runs")
        print("\nMode                                      Median (s)   Range (s)       M iter/s   Build median (s)")
        compile_labels = {
            "code build --release (native)": "code build --release",
            "C (GCC -O2 -march=native)": "GCC -O2 -march=native",
            "C (GCC -O3 -march=native)": "GCC -O3 -march=native",
            "Java 26 (HotSpot default)": "javac --release 26",
        }
        for row in rows:
            compile_label = compile_labels.get(row["mode"])
            if compile_label:
                compile_value = f"{median(compile_samples[compile_label]):.3f}"
            else:
                compile_value = "parse in run"
            print(
                f"{row['mode']:<41} {row['median_seconds']:>8.3f}    "
                f"{min(samples[row['mode']]):.3f}–{max(samples[row['mode']]):.3f}    "
                f"{row['million_iterations_per_second']:>8.2f}        {compile_value}"
            )
        print(f"\nRaw samples: {args.output_dir / 'results.csv'}")
        print(f"Full report: {args.output_dir / 'results.json'}")


if __name__ == "__main__":
    main()

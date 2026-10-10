#!/usr/bin/env python3
"""Compile-time baseline (ticket 115): where `code build` spends its time.

Builds three programs five times each with `--timings` and prints the median
of every stage, as a Markdown table to paste into README.md:

  - the language comparison benchmark (14 lines), native exe, --release
  - todo-api, a held service, shared library, --release
  - aquarium-web, a browser app, wasm, --release

The two apps live in my-euglena-apps; their generated `main.code` is built
directly, with the modules their lock installed in `.code/`.

  benchmarks/compile-time/run.py [--code PATH] [--apps PATH] [--runs N]
"""

import argparse
import re
import statistics
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent

ROW = re.compile(r"^\s+([\d.]+) ms  (.+)$")


def programs(apps: Path):
    return [
        ("sum_squares (14 lines), exe", REPO / "benchmarks/language-comparison", "sum_squares.code", ["--release"]),
        ("todo-api, shared", apps / "todo-api", "main.code", ["--target", "shared", "--release"]),
        ("aquarium-web, wasm", apps / "aquarium-web", "main.code", ["--target", "wasm", "--release"]),
    ]


def stages_of(stderr: str) -> dict:
    found = {}
    for line in stderr.splitlines():
        if line.startswith("IR: "):
            break
        m = ROW.match(line)
        if m:
            found[m.group(2)] = float(m.group(1))
    return found


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--code", default=str(REPO / "target/release/code"))
    parser.add_argument("--apps", default=str(REPO.parent / "my-euglena-apps"))
    parser.add_argument("--runs", type=int, default=5)
    args = parser.parse_args()

    out = Path(tempfile.mkdtemp(prefix="code-compile-time-"))
    print(f"`{subprocess.run([args.code, '--version'], capture_output=True, text=True).stdout.strip()}`, "
          f"median of {args.runs} builds, ms\n")
    for name, cwd, entry, flags in programs(Path(args.apps)):
        if not (cwd / entry).is_file():
            print(f"skipped {name}: no {cwd / entry}", file=sys.stderr)
            continue
        runs = []
        for i in range(args.runs):
            # A clean build every time: the build cache (ticket 118) would
            # otherwise answer runs 2-5 from what run 1 compiled.
            done = subprocess.run(
                [args.code, "build", entry, *flags, "--timings", "-o", str(out / f"artifact-{i}")],
                cwd=cwd, capture_output=True, text=True,
                env={**__import__("os").environ, "CODE_CACHE": "0"},
            )
            if done.returncode != 0:
                print(f"{name} failed:\n{done.stderr}", file=sys.stderr)
                return 1
            runs.append(stages_of(done.stderr))
        order = list(runs[0])
        print(f"**{name}**\n")
        print("| stage | ms |\n|---|---|")
        for stage in order:
            print(f"| {stage} | {statistics.median(r.get(stage, 0.0) for r in runs):.1f} |")
        print()
    return 0


if __name__ == "__main__":
    sys.exit(main())

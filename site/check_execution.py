#!/usr/bin/env python3
"""Verify run/cache/interpret/native/WASM with published Linux Code v2.23.2.

Usage: python3 site/check_execution.py /path/to/code /new/browser-output-dir
Requires curl, cc, wasm-ld and release network access. Projects and cache are
isolated; only the browser proof bundle is retained for real-browser checks.
"""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

SITE = Path(__file__).resolve().parent


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    cli = Path(sys.argv[1]).resolve(strict=True)
    destination = Path(sys.argv[2]).resolve()
    if destination.exists():
        raise SystemExit("Browser output directory must be new")
    source = (SITE / "examples/hello_home.code").read_text()
    with tempfile.TemporaryDirectory(prefix="code-site-execution-") as directory:
        root = Path(directory)
        cache = root / "cache"
        env = dict(os.environ, CODE_CACHE_DIR=str(cache), CODE_CACHE="1",
                   CODE_MODULE_RELEASE="https://github.com/codelovesme/code/releases/tag/v2.23.2")
        for key in ("CODE_RUN", "CODE_LINK", "CODE_MODULE_PATH"):
            env.pop(key, None)

        def run(args, cwd, expected=None):
            result = subprocess.run([str(cli), *args], cwd=cwd, env=env,
                                    capture_output=True, text=True, check=True)
            if expected is not None and result.stdout.strip() != expected:
                raise AssertionError(f"{args}: {result.stdout!r}")
            return result.stdout

        def executables():
            # Cache hits refresh mtime for eviction. Rebuild publication replaces
            # the inode; the bytes and inode must survive an unchanged warm run.
            return {p.name: (p.stat().st_ino, hashlib.sha256(p.read_bytes()).hexdigest())
                    for p in (cache / "run").glob("*") if p.is_file() and not p.suffix}

        if run(["--version"], root).strip() != "Code v2.23.2":
            raise SystemExit("Use the actual published v2.23.2 CLI")
        run(["init", "hello"], root)
        native = root / "hello"
        (native / "main.code").write_text(source)
        run(["install", "console"], native)
        run(["run", "--interpret"], native, "hello, world")
        run(["build", "--release"], native)
        result = subprocess.run([str(native / "build/hello")], cwd=native,
                                env=env, capture_output=True, text=True, check=True)
        if result.stdout.strip() != "hello, world":
            raise AssertionError(result.stdout)

        run(["run"], native, "hello, world")
        cold = executables()
        if len(cold) != 1:
            raise AssertionError("Cold run did not create one native executable")
        run(["run"], native, "hello, world")
        if executables() != cold:
            raise AssertionError("Unchanged warm run rebuilt its native executable")
        (native / "main.code").write_text(source.replace('"world"', '"Ada"'))
        run(["run"], native, "hello, Ada")
        changed = executables()
        if len(changed) != 2:
            raise AssertionError("Changed source did not invalidate the cached build")
        lock = native / ".code/lock.json"
        lock.write_text(lock.read_text() + "\n")
        run(["run"], native, "hello, Ada")
        if len(executables()) <= len(changed):
            raise AssertionError("Changed lockfile did not invalidate the cached build")
        if str(cache) not in run(["cache"], native):
            raise AssertionError("Cache command did not report the isolated cache")

        large = native / "large.code"
        large.write_text(source + "| size probe for the v2.23.2 build threshold\n" * 600)
        run(["run", large.name], native, "hello, world")
        deadline = time.monotonic() + 30
        while True:
            tiers = set()
            for path in (cache / "run").glob("*.inputs"):
                record = dict(line.split("\t", 1) for line in path.read_text().splitlines() if "\t" in line)
                entry = Path(bytes.fromhex(record["entry"]).decode()).resolve()
                if entry == large.resolve():
                    tiers.add(record["tier"])
            if tiers == {"O0", "O2"}:
                break
            if time.monotonic() > deadline:
                raise AssertionError(f"Background optimization tiers: {tiers}")
            time.sleep(.1)

        run(["init", "browser-hello"], root)
        browser = root / "browser-hello"
        (browser / "main.code").write_text(source)
        run(["install", "console", "--platform", "wasm32"], browser)
        run(["build", "main.code", "--target", "wasm", "-o", "web/hello.wasm"], browser)
        shutil.copyfile(SITE / "execution-examples/index.html.txt", browser / "web/index.html")
        shutil.copytree(browser / "web", destination)

    print("Published v2.23.2: interpreted/native output, cold/warm native cache, "
          "source/lock invalidation, quick/background optimized tiers passed")
    print(f"Published CLI/modules produced the browser proof bundle: {destination}")
    print("Serve that directory and verify hello, world in a real browser")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        print(error.stdout or "", end="", file=sys.stderr)
        print(error.stderr or "", end="", file=sys.stderr)
        raise SystemExit(f"Command failed ({error.returncode}): {error.cmd}")

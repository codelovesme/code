#!/usr/bin/env python3
"""Verify the website's native examples with the published Linux CLI.

Usage: python3 site/check_module_examples.py /absolute/path/to/code
Requires Code v2.24.1, curl, a C compiler and network access to its public
release assets. All installs/builds happen in a fresh temporary project.
This does not use source-built modules or modify the user's projects.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

EXAMPLES = Path(__file__).resolve().parent / "module-examples"
RELEASE = "https://github.com/codelovesme/code/releases/tag/v2.24.1"


def main():
    if len(sys.argv) != 2:
        raise SystemExit("Usage: python3 site/check_module_examples.py /absolute/path/to/code")
    cli = Path(sys.argv[1]).resolve(strict=True)
    environment = dict(os.environ, CODE_MODULE_RELEASE=RELEASE)
    environment.pop("CODE_MODULE_PATH", None)
    environment.pop("CODE_RUN", None)
    environment.pop("CODE_LINK", None)

    def run(args, cwd, output=None):
        result = subprocess.run([str(cli), *args], cwd=cwd, env=environment,
                                text=True, capture_output=True, check=True)
        if output is not None and result.stdout.strip() != output:
            raise AssertionError(f"{args}: expected {output!r}, got {result.stdout!r}")
        return result.stdout

    if run(["--version"], EXAMPLES).strip() != "Code v2.24.1":
        raise SystemExit("Use the published v2.24.1 CLI for these versioned examples")

    with tempfile.TemporaryDirectory(prefix="code-site-modules-") as directory:
        root = Path(directory)
        run(["init", "hello"], root)
        hello = root / "hello"
        run(["install", "console"], hello)
        lock = json.loads((hello / ".code/lock.json").read_text())
        console = lock["modules"]["console"]
        if console["version"] != "2.24.1" or console["source"] != RELEASE:
            raise AssertionError(f"Unexpected module provenance: {console}")
        asset = hello / ".code/modules/console" / console["version"] / console["asset"]
        if hashlib.sha256(asset.read_bytes()).hexdigest() != console["sha256"]:
            raise AssertionError("Installed console does not match its lock hash")

        shutil.copyfile(EXAMPLES / "hello/main.code", hello / "main.code")
        run(["format", "--check", "."], hello)
        run(["run", "--interpret"], hello, "hello, modules")
        run(["build", "--release"], hello)
        result = subprocess.run([str(hello / "build/hello")], cwd=hello,
                                text=True, capture_output=True, check=True)
        if result.stdout.strip() != "hello, modules":
            raise AssertionError(result.stdout)

        for source in (EXAMPLES / "reuse").glob("*.code"):
            shutil.copyfile(source, hello / source.name)
        for name, output in [("main", "hello, Ada"), ("team", "hello, Lin")]:
            run(["run", f"{name}.code", "--interpret"], hello, output)
            run(["build", f"{name}.code", "--release", "-o", f"build/{name}"], hello)
            result = subprocess.run([str(hello / f"build/{name}")], cwd=hello,
                                    text=True, capture_output=True, check=True)
            if result.stdout.strip() != output:
                raise AssertionError(result.stdout)

        # Check the private binding is actually refused, not just unused.
        hidden = hello / "private.code"
        hidden.write_text('link "greeter"\nassert greeting = "hello"\n')
        for args in [["run", hidden.name, "--interpret"], ["build", hidden.name]]:
            result = subprocess.run([str(cli), *args], cwd=hello, env=environment,
                                    text=True, capture_output=True)
            if result.returncode == 0 or "undefined variable" not in result.stderr + result.stdout:
                raise AssertionError(f"Private greeting was not refused: {result}")
        hidden.unlink()

        for target, path in [("shared", "libgreeter.so"), ("static", "libgreeter.a")]:
            run(["build", "greeter.code", "--target", target, "-o", path], hello)
        run(["run", "compiled.code", "--interpret"], hello, "hello, Ada")
        run(["build", "compiled.code", "--release", "-o", "build/compiled"], hello)
        result = subprocess.run([str(hello / "build/compiled")], cwd=hello,
                                text=True, capture_output=True, check=True)
        if result.stdout.strip() != "hello, Ada":
            raise AssertionError(result.stdout)
        run(["format", "--check", "."], hello)

    print("Published v2.24.1: clean install/hash, 4 interpreted + 4 native runs, "
          "2 private-name refusals and shared/static library builds passed")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        print(error.stdout or "", end="", file=sys.stderr)
        print(error.stderr or "", end="", file=sys.stderr)
        raise SystemExit(f"Command failed ({error.returncode}): {error.cmd}")

#!/usr/bin/env python3
"""Check the Memory guide with the actual Linux v2.23.0 CLI/modules/header.

Usage: python3 site/check_memory.py /absolute/path/to/code
All projects, downloads and leak-checked native runs use a temporary directory.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import urllib.request

SITE = Path(__file__).resolve().parent


def main():
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    cli = Path(sys.argv[1]).resolve(strict=True)
    demo = next(x for x in json.loads((SITE / "demos.json").read_text()) if x["name"] == "value_snapshot")
    with tempfile.TemporaryDirectory(prefix="code-site-memory-") as directory:
        root = Path(directory)
        env = dict(os.environ, CODE_MODULE_RELEASE="https://github.com/codelovesme/code/releases/tag/v2.23.0",
                   CODE_CACHE_DIR=str(root / "cache"), CODE_CHECK_LEAKS="1")
        for name in ("CODE_RUN", "CODE_MODULE_PATH", "CODE_LINK"):
            env.pop(name, None)

        def run(args, cwd, expected=None):
            p = subprocess.run([str(cli), *args], cwd=cwd, env=env,
                               text=True, capture_output=True, check=True)
            if expected is not None and p.stdout.strip() != expected:
                raise AssertionError(f"{args}: {p.stdout!r}")
            return p.stdout

        if run(["--version"], root).strip() != "Code v2.23.0":
            raise SystemExit("Use the published v2.23.0 CLI")
        run(["init", "memory-demo"], root)
        project = root / "memory-demo"
        run(["install", "console"], project)
        shutil.copyfile(SITE / "examples/value_snapshot.code", project / "main.code")
        run(["format", "--check", "."], project)
        run(["run", "--interpret"], project, demo["output"])
        run(["build", "--release"], project)
        p = subprocess.run([str(project / "build/memory-demo")], cwd=project, env=env,
                           text=True, capture_output=True, check=True)
        if p.stdout.strip() != demo["output"]:
            raise AssertionError(p.stdout)

        # Assert the deeper sharing claims without exposing a large first demo.
        (project / "paths.code").write_text('''original = { user = { name = "Ada" }, tags = ["reader"] }
updated = original
updated.user.name = "Lin"
assert original.user.name = "Ada"
assert updated.user.name = "Lin"
assert original.tags = updated.tags
xs = [1, 2]
saved = xs
xs += 3
assert saved = [1, 2]
assert xs = [1, 2, 3]
word = "hello"
kept = word
word += "!"
assert kept = "hello"
assert word = "hello!"
self ∈ Any = { n = 1 }
self.child = self
assert self.child = { n = 1 }
assert self.child.child = null
''')
        run(["run", "paths.code", "--interpret"], project, "")
        run(["build", "paths.code", "--release", "-o", "build/paths"], project)
        subprocess.run([str(project / "build/paths")], cwd=project, env=env, check=True)

        header = "https://raw.githubusercontent.com/codelovesme/code/v2.23.0/src/code_abi.h"
        with urllib.request.urlopen(header, timeout=30) as response:
            (root / "code_abi.h").write_bytes(response.read())
        (root / "abi.c").write_text('#include "code_abi.h"\n#include <stdio.h>\n'
                                   'int main(void) { printf("%zu %u", sizeof(CodeValue), (unsigned)CODE_ABI_VERSION); }\n')
        subprocess.run(["cc", "-std=c11", "abi.c", "-o", "abi"], cwd=root, check=True)
        result = subprocess.run([str(root / "abi")], capture_output=True, text=True, check=True)
        if result.stdout != "16 2":
            raise AssertionError(f"Published ABI layout: {result.stdout!r}")

    print("Published v2.23.0: visible snapshots, nested paths, arrays/text and self-snapshot "
          "passed interpreted + native, with native leak checks; published ABI slot is 16 bytes (ABI 2)")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        print(error.stdout or "", end="", file=sys.stderr)
        print(error.stderr or "", end="", file=sys.stderr)
        raise SystemExit(f"Command failed ({error.returncode}): {error.cmd}")

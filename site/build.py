#!/usr/bin/env python3
"""Assembles the playground's dist/index.html from the real tests/*.code
fixtures (skipping `fail_*.code` except one, kept to demonstrate error
output) — so the examples shown can never drift from what the language
actually does; run_language_tests.rs already proves every one of them
either runs or fails as claimed.

Usage: build.py <repo-root> <dist-dir>
"""
import json
import sys
from pathlib import Path

def main() -> None:
    repo_root = Path(sys.argv[1])
    dist_dir = Path(sys.argv[2])
    tests_dir = repo_root / "tests"
    site_dir = repo_root / "site"

    examples = {}
    for path in sorted(tests_dir.glob("*.code")):
        name = path.stem
        if name.startswith("fail_") and name != "fail_undefined_variable":
            continue
        examples[name] = path.read_text()

    # Escaping "</" guards against a fixture ever containing a "</script>"
    # substring, which would otherwise close the embedding <script> tag
    # early and corrupt the page.
    examples_json = json.dumps(examples).replace("</", "<\\/")

    template = (site_dir / "index.html").read_text()
    page = template.replace("__EXAMPLES__", examples_json)

    dist_dir.mkdir(parents=True, exist_ok=True)
    (dist_dir / "index.html").write_text(page)

if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Assembles the playground's dist/index.html from the real tests/*.code
fixtures — so the examples shown can never drift from what the language
actually does; run_language_tests.rs already proves every one of them
either runs or fails as claimed.

Two prefixes are held back. `fail_*` because a page of error messages is
not an introduction to the language (one is kept, to show what an error
looks like). `stress_*` because those fixtures are generated, thousands of
lines of `a = a + a`, and exist to prove things about the *native* backend
— stack growth, teardown depth — which is exactly what a wasm playground
cannot demonstrate.

Fixtures that `link` are held back too, by content rather than by name:
the playground runs through `crates/code-wasm`, which has no filesystem and
so deliberately supports no modules at all. Testing the source is what makes
this precise — it holds back exactly the examples that would fail to run,
and stays right no matter what the fixtures are called.

Usage: build.py <repo-root> <dist-dir>
"""
import json
import shutil
import sys
from pathlib import Path

# Ordered: first matching prefix wins. `fail_`/`emit_`/`particle_` are the
# most specific and must be checked before the generic value prefixes.
CATEGORY_BY_PREFIX = [
    ("fail_", "Errors"),
    ("emit_", "Particles"),
    ("particle_", "Particles"),
    ("loop_", "Loops"),
    ("if_", "Conditionals"),
    ("block_", "Scoping"),
    ("assert_", "Assertions"),
    ("let_", "Values & Expressions"),
    ("variables_", "Values & Expressions"),
    ("literal_", "Values & Expressions"),
    ("array_", "Values & Expressions"),
    ("object_", "Values & Expressions"),
    ("nested_", "Values & Expressions"),
    ("multiline_", "Values & Expressions"),
    ("string_", "Values & Expressions"),
    ("arithmetic_", "Values & Expressions"),
    ("comparison_", "Values & Expressions"),
    ("inequality_", "Values & Expressions"),
    ("logical_", "Values & Expressions"),
    ("unary_", "Values & Expressions"),
    ("operator_", "Values & Expressions"),
    ("parens_", "Values & Expressions"),
    ("field_", "Values & Expressions"),
    ("index_", "Values & Expressions"),
    ("chained_", "Values & Expressions"),
    ("invalid_", "Values & Expressions"),
    ("values_", "Values & Expressions"),
    ("reassignment_", "Values & Expressions"),
    ("self_", "Values & Expressions"),
]
DEFAULT_CATEGORY = "Values & Expressions"


def category_for(name: str) -> str:
    for prefix, cat in CATEGORY_BY_PREFIX:
        if name.startswith(prefix):
            return cat
    return DEFAULT_CATEGORY


def first_comment(source: str) -> str:
    """The fixture's first `--` comment line, stripped — the author's own
    one-line explanation of the feature."""
    for line in source.splitlines():
        s = line.strip()
        if s.startswith("--"):
            text = s[2:].strip()
            if text:
                return text
    return ""


def truncate(text: str, limit: int = 110) -> str:
    if len(text) <= limit:
        return text
    cut = text[:limit].rsplit(" ", 1)[0]
    return cut.rstrip(",;: ") + "…"


def main() -> None:
    repo_root = Path(sys.argv[1])
    dist_dir = Path(sys.argv[2])
    tests_dir = repo_root / "tests"
    site_dir = repo_root / "site"

    examples = []
    for path in sorted(tests_dir.glob("*.code")):
        name = path.stem
        if name.startswith("fail_") and name != "fail_undefined_variable":
            continue
        if name.startswith("stress_"):
            continue
        source = path.read_text()
        # `link` is top-level only, so a line starting with it is the whole
        # of the syntax to look for.
        if any(line.startswith("link ") for line in source.splitlines()):
            continue
        examples.append(
            {
                "name": name,
                "category": category_for(name),
                "description": truncate(first_comment(source)),
                "code": source,
            }
        )

    # Escaping "</" guards against a fixture ever containing a "</script>"
    # substring, which would otherwise close the embedding <script> tag
    # early and corrupt the page.
    examples_json = json.dumps(examples).replace("</", "<\\/")

    template = (site_dir / "index.html").read_text()
    page = template.replace("__EXAMPLES__", examples_json)

    dist_dir.mkdir(parents=True, exist_ok=True)
    (dist_dir / "index.html").write_text(page)

    # Static assets (logo, favicon) live next to index.html in site/ and are
    # copied through verbatim — the page references them by relative path.
    for asset in sorted(site_dir.glob("*.png")):
        shutil.copy2(asset, dist_dir / asset.name)

    # The first-party module index (`code module install` fetches it from here —
    # see src/main.rs's MODULE_INDEX_URL). It lives at the repo root because
    # it documents releases across the whole repo, not the playground.
    index_src = repo_root / "modules-index.json"
    if index_src.is_file():
        shutil.copy2(index_src, dist_dir / index_src.name)

if __name__ == "__main__":
    main()

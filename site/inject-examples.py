#!/usr/bin/env python3
"""Inject the CI-verified docs/examples/*.code files into site/guide.html.

The guide's code blocks must never drift from the real programs CI runs. So
the template ships with `__EXAMPLE__<slug>__` placeholders instead of copied
snippets, and this script substitutes each one with the exact (HTML-escaped)
contents of the corresponding .code file at site-assembly time. If a
referenced file is missing, or a placeholder goes unfilled, this fails loudly
rather than publishing a broken or stale page.

Usage: inject-examples.py <guide-template.html> <examples-dir> <output.html>
"""
import html
import pathlib
import re
import sys


def slug_to_relpath(slug: str) -> str:
    # Placeholders spell paths with `_`; example files use `-` and a `/`
    # only for the modules subdir. `01_getting_started` -> `01-getting-
    # started.code`; `modules_handler_module` -> `modules/handler-module.code`.
    if slug.startswith("modules_"):
        return "modules/" + slug[len("modules_") :].replace("_", "-") + ".code"
    return slug.replace("_", "-") + ".code"


def main() -> int:
    if len(sys.argv) != 4:
        print(__doc__, file=sys.stderr)
        return 2
    template_path, examples_dir, out_path = sys.argv[1:4]
    template = pathlib.Path(template_path).read_text(encoding="utf-8")
    examples = pathlib.Path(examples_dir)

    placeholder = re.compile(r"__EXAMPLE__([a-z0-9_]+)__")
    missing: list[str] = []

    def replace(match: "re.Match[str]") -> str:
        slug = match.group(1)
        f = examples / slug_to_relpath(slug)
        if not f.is_file():
            missing.append(f"{slug} -> {f}")
            return match.group(0)
        # Strip a single trailing newline so the <code> block has no blank
        # last line, then HTML-escape (the .code files use <, >, & freely).
        return html.escape(f.read_text(encoding="utf-8").rstrip("\n"))

    result = placeholder.sub(replace, template)

    if missing:
        print("error: could not resolve example files:", file=sys.stderr)
        for m in missing:
            print(f"  {m}", file=sys.stderr)
        return 1

    leftover = placeholder.search(result)
    if leftover:
        print(f"error: unfilled placeholder remains: {leftover.group(0)}", file=sys.stderr)
        return 1

    pathlib.Path(out_path).write_text(result, encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())

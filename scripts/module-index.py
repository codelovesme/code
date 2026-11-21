#!/usr/bin/env python3
"""One file that says what a release's modules are: modules-index.json.

Usage: module-index.py <dist-dir> <repo-root> > modules-index.json

`<dist-dir>` holds the release's own `<module>.json` files (what
publish-modules writes and attaches); `<repo-root>` is the source the
release was built from, for what a person reads — each module's one-line
description (its Cargo.toml) and its README. The site's module catalog is
drawn from this file alone, so it lists exactly what a release offers and
needs nothing at the moment someone reads it: no backend, no API calls.
"""
import json
import re
import sys
from pathlib import Path

# A README is shown whole; a very long one is cut rather than making the
# index heavy for everyone who opens the catalog.
README_AT_MOST = 24_000

# Not modules: files publish-modules writes beside them.
NOT_MODULES = {"module-hashes", "modules-index"}


def description_of(crate: Path) -> str:
    toml = crate / "Cargo.toml"
    if not toml.exists():
        return ""
    found = re.search(r'^description\s*=\s*"((?:[^"\\]|\\.)*)"', toml.read_text(), re.M)
    if not found:
        return ""
    text = found.group(1).replace('\\"', '"')
    # "The `x` native module for the Code programming language — what it does.
    # Written in Rust on code-native." → "what it does."
    text = re.sub(r"\s*Written in [^.]*\.\s*$", "", text)
    lead = re.match(r"The `[^`]+` (?:native )?module for (?:the )?Code(?: programming language)?\s*[—-]\s*", text)
    if lead:
        text = text[lead.end():]
        text = text[:1].upper() + text[1:]
    return text.strip()


# What a machine half says when the module only works in a page.
BROWSER_ONLY_MARKS = ("no browser here", "there is no page", "no page here", "NO_BROWSER", "NO_PAGE")


def runs_on(crate: Path) -> str:
    """"browser", "machine" or "both", from the module's own two halves."""
    if not (crate / "page.mjs").exists():
        return "machine"
    lib = crate / "src" / "lib.rs"
    text = lib.read_text() if lib.exists() else ""
    return "browser" if any(mark in text for mark in BROWSER_ONLY_MARKS) else "both"


def readme_of(crate: Path) -> str:
    readme = crate / "README.md"
    if not readme.exists():
        return ""
    text = readme.read_text()
    if len(text) > README_AT_MOST:
        text = text[:README_AT_MOST].rsplit("\n", 1)[0] + "\n\n…"
    return text


def main() -> None:
    dist, root = Path(sys.argv[1]), Path(sys.argv[2])
    modules = []
    version = None
    for path in sorted(dist.glob("*.json")):
        if path.stem in NOT_MODULES:
            continue
        info = json.loads(path.read_text())
        if not isinstance(info, dict) or "platforms" not in info:
            continue
        name = info["name"]
        version = version or info.get("version")
        crate = root / "crates" / "modules" / name
        platforms = sorted(info["platforms"].keys())
        modules.append({
            "name": name,
            "version": info.get("version"),
            "description": description_of(crate),
            "handlers": info.get("handlers", []),
            "setup": info.get("setup"),
            "platforms": platforms,
            "runs": runs_on(crate),
            "test_double": name.endswith("_mock"),
            "readme": readme_of(crate),
        })
    if not modules:
        sys.exit("module-index: no <module>.json in " + str(dist))
    json.dump({"version": version, "count": len(modules), "modules": modules}, sys.stdout, indent=1, ensure_ascii=False)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()

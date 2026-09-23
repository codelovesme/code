#!/usr/bin/env python3
"""Moves everything this repository publishes to a new version, in one go.

    scripts/bump-version.py 2.9.0
    scripts/bump-version.py 3.0.0 --major    # only when the owner said so

One version for the CLI, `code-native`, `code-wasm`, `code-lsp`, the editor
extension and every module (see `tests/one_version.rs`) means a release
touches about ninety files. By hand that is the step that goes wrong: a tag
cut on an unbumped tree ships a release with no module assets, and a
`Cargo.lock` entry missed is a build that re-resolves on someone else's
machine. This rewrites:

  - the package `version` of every `Cargo.toml` that is on the old version
    (root, `crates/*`, `crates/modules/*`);
  - `"version"` in `editor/vscode/package.json` and
    `crates/code-wasm/npm/package.json`;
  - in every `Cargo.lock`, the entries for our own crates only — a package
    with no `source` line (a path dependency) on the old version. A
    third-party crate that happens to share the number has a `source` and is
    left alone.

Then it runs `cargo test --test one_version` (skip with `--no-test`).
It refuses a new major version without `--major`, and refuses to go
backwards. `old/` and `templates/` are not published and are not touched.
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PACKAGE_JSONS = ["editor/vscode/package.json", "crates/code-wasm/npm/package.json"]


def current_version() -> str:
    text = (ROOT / "Cargo.toml").read_text()
    match = re.search(r'^version = "([^"]+)"', text, re.M)
    if not match:
        sys.exit("no version in Cargo.toml")
    return match.group(1)


def parse(version: str) -> tuple[int, int, int]:
    match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", version)
    if not match:
        sys.exit(f"not a version: {version} (want X.Y.Z)")
    return tuple(int(part) for part in match.groups())


def repo_files(suffix: str) -> list[Path]:
    """Tracked and untracked-but-not-ignored files — a module added and not
    yet committed is published with the rest, so it moves with the rest."""
    listed = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.split("\n")
    return [
        ROOT / path
        for path in listed
        if path.endswith(suffix)
        and not path.startswith(("old/", "templates/"))
        and (ROOT / path).is_file()
    ]


def bump_manifest(path: Path, old: str, new: str) -> bool:
    text = path.read_text()
    # The first `version =` is the package's own — it precedes every
    # dependency's, which is the same rule `one_version.rs` reads it by.
    match = re.search(r'^version = "([^"]+)"', text, re.M)
    if not match or match.group(1) != old:
        return False
    path.write_text(text[: match.start()] + f'version = "{new}"' + text[match.end() :])
    return True


def bump_lock(path: Path, old: str, new: str) -> bool:
    text = path.read_text()
    blocks = text.split("\n[[package]]\n")
    changed = False
    for i, block in enumerate(blocks):
        if i == 0 or "\nsource = " in f"\n{block}":
            continue
        replaced = re.sub(rf'^version = "{re.escape(old)}"$', f'version = "{new}"', block, count=1, flags=re.M)
        if replaced != block:
            blocks[i] = replaced
            changed = True
    if changed:
        path.write_text("\n[[package]]\n".join(blocks))
    return changed


def bump_package_json(path: Path, old: str, new: str) -> bool:
    text = path.read_text()
    replaced = re.sub(rf'^(\s*"version": )"{re.escape(old)}"', rf'\g<1>"{new}"', text, count=1, flags=re.M)
    if replaced == text:
        return False
    path.write_text(replaced)
    return True


def main() -> None:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    flags = {a for a in sys.argv[1:] if a.startswith("--")}
    unknown = flags - {"--major", "--no-test"}
    if len(args) != 1 or unknown:
        sys.exit(__doc__.split("\n\n")[1])
    new = args[0].removeprefix("v")
    old = current_version()

    if parse(new) <= parse(old):
        sys.exit(f"{new} is not after the current {old}")
    if parse(new)[0] != parse(old)[0] and "--major" not in flags:
        sys.exit(
            f"{old} -> {new} is a new major version. Breaking changes still go out "
            "as a minor unless the owner says otherwise; pass --major if they did."
        )

    changed = []
    for path in repo_files("Cargo.toml"):
        if bump_manifest(path, old, new):
            changed.append(path)
    for path in repo_files("Cargo.lock"):
        if bump_lock(path, old, new):
            changed.append(path)
    for rel in PACKAGE_JSONS:
        path = ROOT / rel
        if not bump_package_json(path, old, new):
            sys.exit(f"{rel} is not on {old}; fix it by hand, then run again")
        changed.append(path)

    print(f"{old} -> {new}: {len(changed)} files")
    if "--no-test" not in flags:
        subprocess.run(["cargo", "test", "--quiet", "--test", "one_version"], cwd=ROOT, check=True)
    print(f"Next: commit, push, check CI is green, then `git tag -a v{new}`.")


if __name__ == "__main__":
    main()

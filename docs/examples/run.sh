#!/usr/bin/env bash
# Run every documentation example and fail if any doesn't execute cleanly.
#
# Docs examples are exactly where language drift silently rots content: a
# syntax change lands and nobody notices a guide now shows something that no
# longer parses. Every .code file here is a real, executed program — not a
# prose-embedded snippet — verified by this script in CI (see T22).
#
# Usage: docs/examples/run.sh [path-to-code-binary]
#   Defaults to ./target/debug/code. Handler-less files (the linked module
#   in modules/, tutorial/validate-basic.code, and reference/module-helper.code)
#   are skipped as entry points — they're exercised via the files that link
#   them (07-modules.code; tutorial/03-modules.code and 04-batch.code;
#   reference/modules.code).
set -euo pipefail

code_bin="${1:-./target/debug/code}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ ! -x "$code_bin" ]]; then
  echo "error: code binary not found or not executable: $code_bin" >&2
  exit 1
fi

fail=0
run_one() {
  local f="$1"
  if "$code_bin" run "$f" >/dev/null 2>&1; then
    echo "PASS  ${f#"$here"/}"
  else
    echo "FAIL  ${f#"$here"/}"
    "$code_bin" run "$f" 2>&1 | sed 's/^/      /' || true
    fail=1
  fi
}

# Guide examples: top-level numbered files only — modules/ holds linked
# dependencies, not entry points (07-modules.code links and drives it).
for f in "$here"/[0-9]*.code; do
  run_one "$f"
done

# Tutorial examples: same convention, one directory over — numbered entry
# points only, validate-basic.code is a linked dependency, not an entry.
for f in "$here"/tutorial/[0-9]*.code; do
  run_one "$f"
done

# Reference examples: independent topics, not sequential steps, so no
# numbered-prefix convention — every file is an entry point except the
# one linked dependency.
for f in "$here"/reference/*.code; do
  [[ "$(basename "$f")" == "module-helper.code" ]] && continue
  run_one "$f"
done

exit "$fail"

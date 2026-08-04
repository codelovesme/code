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
#   in modules/) are skipped as entry points — they're exercised via 07.
set -euo pipefail

code_bin="${1:-./target/debug/code}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ ! -x "$code_bin" ]]; then
  echo "error: code binary not found or not executable: $code_bin" >&2
  exit 1
fi

fail=0
# Top-level examples only — modules/ holds linked dependencies, not entry
# points (07-modules.code links and drives them).
for f in "$here"/[0-9]*.code; do
  if "$code_bin" run "$f" >/dev/null 2>&1; then
    echo "PASS  ${f#"$here"/}"
  else
    echo "FAIL  ${f#"$here"/}"
    "$code_bin" run "$f" 2>&1 | sed 's/^/      /' || true
    fail=1
  fi
done

exit "$fail"

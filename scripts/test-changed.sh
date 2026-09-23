#!/usr/bin/env bash
# Runs the tests a change can affect, and no more.
#
#   scripts/test-changed.sh            # against origin/main
#   scripts/test-changed.sh v2.8.2     # against any ref
#   scripts/test-changed.sh -n         # only say what would run
#
# What changed (committed since the ref, staged, unstaged and untracked)
# decides what runs:
#
#   crates/modules/<m>/**   build <m>, clippy it, run every fixture that links
#                           it or is named after it, and every Rust test that
#                           names it
#   tests/<fixture>.code    run that fixture
#   tests/<name>.rs         run that test binary
#   docs, *.md              nothing
#   anything else           the whole suite: `cargo test --workspace`
#
# "Anything else" is on purpose. The compiler, `code-native`, the runtime and
# the ABI are shared by every module, and a change to a shared type once
# shipped green from a partial run (see AGENTS.md) — so those always get the
# lot. This is the quick loop for module work; CI still runs everything.
#
# The cheap guards (`one_version`, `first_party_modules`) run every time.
set -euo pipefail

cd "$(dirname "$0")/.."
root="$PWD"
dry=0
if [[ "${1:-}" == -n ]]; then dry=1; shift; fi
base="${1:-origin/main}"
# `run cmd...` runs it, or with -n prints it.
run() {
  if (( dry )); then printf '  %s\n' "$*"; else "$@"; fi
}

changed="$( {
  git diff --name-only "$base" --
  git ls-files --others --exclude-standard
} | sort -u )"

if [[ -z "$changed" ]]; then
  echo "Nothing changed against $base."
  exit 0
fi

modules=()
fixtures=()
tests=()
full=0
while IFS= read -r path; do
  case "$path" in
    crates/modules/*/*)
      m="${path#crates/modules/}"; m="${m%%/*}"
      [[ -f "crates/modules/$m/Cargo.toml" ]] && modules+=("$m") ;;
    tests/*.code) fixtures+=("$(basename "$path")") ;;
    tests/support/*) full=1 ;;
    tests/*.rs) tests+=("$(basename "$path" .rs)") ;;
    docs/*|*.md|scripts/*) ;;
    *) full=1; echo "shared file changed: $path" ;;
  esac
done <<<"$changed"

if (( full )); then
  echo "==> full suite"
  run cargo test --workspace
  exit
fi

mapfile -t modules < <(printf '%s\n' "${modules[@]}" | sort -u | sed '/^$/d')
for m in "${modules[@]}"; do
  # Warnings are shown, not fatal: CI holds only the main workspace to
  # `-D warnings`, and several modules carry lints that predate this script.
  echo "==> clippy $m"
  run env -C "crates/modules/$m" CARGO_TARGET_DIR="$root/target/modules" \
    cargo clippy --release --all-targets
  # Fixtures named after the module, and fixtures that link it.
  for f in tests/"${m}"_*.code; do
    if [[ -e "$f" ]]; then fixtures+=("$(basename "$f")"); fi
  done
  while IFS= read -r f; do fixtures+=("$(basename "$f")"); done \
    < <(grep -lE "native_modules/${m}\.(so|a)\b" tests/*.code || true)
  # Rust tests that build it by name. Not the fixture runner, which names
  # every module — its share is the fixtures above.
  while IFS= read -r f; do tests+=("$(basename "$f" .rs)"); done \
    < <(grep -lE "\"${m}\"|/${m}\.(so|a)\b" tests/*.rs | grep -v run_language_tests || true)
done

tests+=(one_version first_party_modules)
mapfile -t tests < <(printf '%s\n' "${tests[@]}" | sort -u | sed '/^$/d')
mapfile -t fixtures < <(printf '%s\n' "${fixtures[@]}" | sort -u | sed '/^$/d')

args=()
for t in "${tests[@]}"; do
  [[ "$t" == run_language_tests ]] && continue
  [[ -f "tests/$t.rs" ]] && args+=(--test "$t")
done

if (( ${#fixtures[@]} )); then
  echo "==> ${#fixtures[@]} fixture(s)"
  run env FIXTURES="$(IFS=,; echo "${fixtures[*]}")" cargo test --test run_language_tests
elif printf '%s\n' "${tests[@]}" | grep -qx run_language_tests; then
  echo "==> every fixture"
  run cargo test --test run_language_tests
fi

echo "==> ${args[*]}"
run cargo test "${args[@]}"

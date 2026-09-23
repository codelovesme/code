#!/usr/bin/env bash
# One line per module in crates/modules/: its name, a fingerprint of its
# source, and whether it has a browser half (1 when it carries a page.mjs).
#
#   console 3f9a0c1d2e4b5a67 1
#
# The release reuses a module's previous artifact when the fingerprint is the
# one the previous release recorded (publish-modules.yml's `plan` job), so
# the fingerprint has to cover everything the artifact is built from:
#
#   - every file of the module, except target/;
#   - every file of crates/code-native, which every module links;
#   - src/ and the root Cargo.toml, for the modules that depend on the
#     compiler itself (`path = "../../.."` — interpreter, syntax).
#
# The repository version is blanked out before hashing: a release bumps it in
# every Cargo.toml and Cargo.lock, and a number changing is not the module
# changing. Nothing a module builds embeds it (no CARGO_PKG_VERSION in
# crates/modules or crates/code-native).
#
# Tracked and untracked-but-not-ignored files both count, so a run on a dirty
# tree fingerprints what is actually there.
set -euo pipefail

cd "$(dirname "$0")/.."
version="$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)"

files() {
  git ls-files --cached --others --exclude-standard -- "$@" | sort
}

# A file's contents with the repository version blanked — only where a bump
# writes it (scripts/bump-version.py): a Cargo.toml's own package version,
# and a Cargo.lock's entries for our own crates, the ones with no `source`.
# A third-party crate that happens to share the number is left as it is.
blanked() {
  local f="$1" line="version = \"$version\""
  case "$f" in
    */Cargo.toml|Cargo.toml)
      awk -v line="$line" '!done && $0 == line { print "version = \"\""; done = 1; next } { print }' "$f" ;;
    */Cargo.lock)
      awk -v line="$line" '
        function flush() {
          for (i = 1; i <= n; i++) print (!src && buf[i] == line) ? "version = \"\"" : buf[i]
          n = 0; src = 0
        }
        /^\[\[package\]\]$/ { flush() }
        { buf[++n] = $0; if ($0 ~ /^source = /) src = 1 }
        END { flush() }' "$f" ;;
    *) cat "$f" ;;
  esac
}

# Reads paths on stdin; prints one hash over their names and contents.
fingerprint() {
  while IFS= read -r f; do
    [[ -f "$f" ]] || continue
    printf '%s %s\n' "$f" "$(blanked "$f" | sha256sum | cut -d' ' -f1)"
  done | sha256sum | cut -d' ' -f1
}

native="$(files crates/code-native | fingerprint)"
compiler="$(files src Cargo.toml | fingerprint)"

for dir in crates/modules/*/; do
  m="$(basename "$dir")"
  [[ -f "$dir/Cargo.toml" ]] || continue
  deps="$native"
  if grep -q 'path = "../../.."' "$dir/Cargo.toml"; then
    deps="$deps $compiler"
  fi
  own="$(files "$dir" | fingerprint)"
  hash="$(printf '%s %s\n' "$own" "$deps" | sha256sum | cut -c1-16)"
  wasm=0
  [[ -f "$dir/page.mjs" ]] && wasm=1
  echo "$m $hash $wasm"
done

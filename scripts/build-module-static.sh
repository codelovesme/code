#!/usr/bin/env bash
# Builds one native module as a static archive (ticket 141):
#
#   scripts/build-module-static.sh <module> [out-dir]   (default dist/)
#
# into <out-dir>/<module>-linux-x86_64.a, with beside it
# <out-dir>/<module>-linux-x86_64.a.libs: the system libraries the archive
# needs at the final link, one linker flag per line.
#
# The module's own source is not touched: it exports `code_module_*` once,
# for its `.so` (scripts/build-module-linux.sh). Here it is built as a
# `staticlib` without a runtime of its own (`code-native/static-module`; the
# program it links into supplies the one runtime), and then:
#
#   - merged into one object (`ld -r`), its exports renamed
#     `<module>_code_module_*` — the prefix `code build` finds a `.a`
#     module's entry points by (code_abi.h, ".a static modules");
#   - every other symbol it defines made local to that object. Each Rust
#     module carries its own copy of the standard library and of its
#     dependencies; two of them in one program would otherwise define the
#     same names. Kept local, each module's copy is its own and nothing
#     collides. The runtime's functions and the system's stay undefined
#     here and are found at the final link.
set -euo pipefail
m="$1"
out="${2:-dist}"
cd "$(dirname "$0")/.."
root="$PWD"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

notes="$work/notes.txt"
(cd "crates/modules/$m" \
  && cargo rustc --release --lib --crate-type staticlib \
       --features code-native/static-module \
       --target-dir target/static -- --print native-static-libs) 2> "$notes" \
  || { cat "$notes" >&2; exit 1; }
archive="crates/modules/$m/target/static/release/lib$m.a"

# The libraries rustc says a staticlib of this crate needs, as linker flags.
libs="$(sed -n 's/.*native-static-libs: //p' "$notes" | tail -1)"

# Only what the module's exports reach: each `code_module_*` it defines is
# made a root (`-u`), and the archive members those need are pulled in, the
# way a final link pulls them — not the whole standard library.
nm --defined-only -g "$archive" 2>/dev/null | awk 'NF >= 3 {print $NF}' | grep '^code_module_' | sort -u > "$work/exports" \
  || { echo "build-module-static: $m defines no code_module_* export" >&2; exit 1; }
roots=()
while read -r sym; do roots+=(-u "$sym"); done < "$work/exports"
ld -r --gc-sections "${roots[@]}" "$archive" -o "$work/merged.o"
renames=()
: > "$work/keep"
while read -r sym; do
  renames+=(--redefine-sym "$sym=${m}_$sym")
  echo "${m}_$sym" >> "$work/keep"
done < "$work/exports"
objcopy "${renames[@]}" "$work/merged.o" "$work/renamed.o"
# No debug information either, as for the .so (ticket 176), and no embedded
# LLVM bitcode (`.llvmbc`, Rust's for link-time optimisation, which nothing
# here does): together they were 97% of an archive, and the final link's
# --gc-sections drops neither.
objcopy --strip-debug --remove-section='.llvmbc*' --remove-section='.llvmcmd*' \
  --keep-global-symbols="$work/keep" "$work/renamed.o" "$work/$m.o"

mkdir -p "$out"
rm -f "$out/${m}-linux-x86_64.a"
ar rcs "$out/${m}-linux-x86_64.a" "$work/$m.o"
tr ' ' '\n' <<<"$libs" | grep -v '^$' > "$out/${m}-linux-x86_64.a.libs" || true
echo "$out/${m}-linux-x86_64.a"

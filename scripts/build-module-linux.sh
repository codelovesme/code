#!/usr/bin/env bash
# Builds one native module the way a release publishes it:
#
#   scripts/build-module-linux.sh <module> [out-dir]   (default dist/)
#
# into <out-dir>/<module>-linux-x86_64.so, stripped. Each link of a module
# loads its own copy of the whole file into memory (runtime.c
# `module_image`), so the file's size is paid once per link: mongodb was
# 12.5 MB, 9.3 MB stripped, and the live host held 16 copies (ticket 176).
# `--strip-unneeded` drops the debug information and the static symbol
# table; the dynamic symbols the loader looks up stay.
#
# scripts/module-hashes.sh fingerprints this file too: a change here is a
# change to every module's artifact.
set -euo pipefail
m="$1"
out="${2:-dist}"
cd "$(dirname "$0")/.."
(cd "crates/modules/$m" && cargo build --release)
mkdir -p "$out"
cp "crates/modules/$m/target/release/lib$m.so" "$out/${m}-linux-x86_64.so"
strip --strip-unneeded "$out/${m}-linux-x86_64.so"

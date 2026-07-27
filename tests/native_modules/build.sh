#!/usr/bin/env bash
# Builds the native-module test fixtures used by `code test`'s native_*.code cases.
# These are intentionally NOT committed as binaries (they're build artifacts); run this
# once (or via CI) before `code test` if these tests are needed.
#
# Requires: cc (or gcc/clang), rustc, clang + wasm-ld (for the .wasm fixture).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PROFILE="${1:-debug}"

CODE_NATIVE_RLIB=$(find "$REPO_ROOT/target/$PROFILE/deps" -maxdepth 1 -name 'libcode_native-*.rlib' | head -1)
if [[ -z "$CODE_NATIVE_RLIB" ]]; then
    echo "error: libcode_native rlib not found under target/$PROFILE/deps — run 'cargo build --workspace' first" >&2
    exit 1
fi

echo "== building test_math.c -> libtest_math.so =="
cc -shared -fPIC -I"$SCRIPT_DIR" -o "$SCRIPT_DIR/libtest_math.so" "$SCRIPT_DIR/test_math.c"

echo "== building test_strings.rs -> libtest_strings.so =="
rustc --edition 2021 --crate-type cdylib -o "$SCRIPT_DIR/libtest_strings.so" "$SCRIPT_DIR/test_strings.rs"

echo "== building console.rs -> console.so =="
rustc --edition 2021 --crate-type cdylib \
    --extern code_native="$CODE_NATIVE_RLIB" \
    -L "$REPO_ROOT/target/$PROFILE/deps" \
    -o "$SCRIPT_DIR/console.so" "$SCRIPT_DIR/console.rs"

echo "== building test_helper.rs -> libtest_helper.so =="
rustc --edition 2021 --crate-type cdylib \
    --extern code_native="$CODE_NATIVE_RLIB" \
    -L "$REPO_ROOT/target/$PROFILE/deps" \
    -o "$SCRIPT_DIR/libtest_helper.so" "$SCRIPT_DIR/test_helper.rs"

echo "== building test_math_wasm.c -> test_math.wasm =="
WASM_LD=""
for candidate in wasm-ld wasm-ld-17 wasm-ld-18 wasm-ld-19; do
    if command -v "$candidate" >/dev/null 2>&1; then WASM_LD="$candidate"; break; fi
    for p in /usr/lib/llvm-*/bin/"$candidate"; do
        [[ -x "$p" ]] && { WASM_LD="$p"; break 2; }
    done
done
if [[ -z "$WASM_LD" ]]; then
    echo "warning: wasm-ld not found — skipping test_math.wasm (native_link_wasm.code will fail)" >&2
else
    clang --target=wasm32 -nostdlib -O2 -I"$SCRIPT_DIR" \
        -Wl,--no-entry -Wl,--export-all -Wl,--allow-undefined -fuse-ld="$WASM_LD" \
        -o "$SCRIPT_DIR/test_math.wasm" "$SCRIPT_DIR/test_math_wasm.c"
fi

echo "done — native_modules test fixtures built."

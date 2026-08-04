#!/usr/bin/env bash
# Builds code-wasm for wasm32 (release — a debug build crashes rust-lld at
# this crate's size; see ../README.md) and runs wasm-bindgen to produce
# browser-loadable glue into ./pkg (gitignored build output — same
# not-committed-binaries convention as tests/native_modules/build.sh).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

cd "$REPO_ROOT"
echo "== building code-wasm (release, wasm32-unknown-unknown) =="
cargo build -p code-wasm --target wasm32-unknown-unknown --release

# wasm-bindgen-cli's version must exactly match the wasm-bindgen crate
# version resolved in Cargo.lock, or glue generation errors out.
WASM_BINDGEN_VERSION=$(cargo pkgid wasm-bindgen | sed 's/.*@//')
INSTALLED_VERSION="$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)"
if [[ "$INSTALLED_VERSION" != "$WASM_BINDGEN_VERSION" ]]; then
    echo "== installing wasm-bindgen-cli $WASM_BINDGEN_VERSION (matching the wasm-bindgen crate) =="
    cargo install wasm-bindgen-cli --version "$WASM_BINDGEN_VERSION" --locked
fi

echo "== generating browser glue into playground/pkg =="
wasm-bindgen --target web --out-dir "$SCRIPT_DIR/pkg" \
    target/wasm32-unknown-unknown/release/code_wasm.wasm

cat <<EOF

done — playground/pkg/ ready.

ES modules require a real HTTP server (file:// won't work for the import).
Serve this directory and open index.html, e.g.:
  cd $SCRIPT_DIR && python3 -m http.server 8000
EOF

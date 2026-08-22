#!/usr/bin/env bash
# Builds code-wasm for wasm32 (release) and runs wasm-bindgen to produce the
# npm-publishable glue into ./dist (gitignored build output — see
# .gitignore). Mirrors .github/workflows/pages.yml's own build steps
# exactly except for the output directory: `--target web` both here and
# there, so the published package is the *same* artifact the docs-site
# playground can consume.
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

echo "== generating npm package glue into npm/dist =="
rm -rf "$SCRIPT_DIR/dist"
wasm-bindgen --target web --out-dir "$SCRIPT_DIR/dist" \
    target/wasm32-unknown-unknown/release/code_wasm.wasm

cat <<EOF

done — npm/dist/ ready.

Verify before publishing:
  cd $SCRIPT_DIR && npm pack --dry-run
  node smoke-test.mjs

Publish (requires npm auth):
  cd $SCRIPT_DIR && npm publish
EOF

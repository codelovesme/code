#!/usr/bin/env bash
# Builds code-wasm for wasm32 (release — a debug build crashes rust-lld at
# this crate's size; see ../README.md) and runs wasm-bindgen to produce the
# npm-publishable glue into ./dist (gitignored build output — same
# not-committed-binaries convention as playground/build.sh, which this
# mirrors exactly except for the output directory and target: `--target web`
# both here and in the playground, so the published package is the *same*
# artifact our own docs-site playground can consume — see T19's "dogfood the
# public contract" acceptance criterion).
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

Publish (requires npm auth — see README.md "Releasing"):
  cd $SCRIPT_DIR && npm publish
EOF

#!/usr/bin/env sh
# Install the `code` SDK (compiler + interpreter) from GitHub Releases.
#
#   curl -sSf https://raw.githubusercontent.com/codelovesme/code/main/install.sh | sh
#
# Installs the SDK tier — the full `code` with `build` (native/wasm codegen).
# Editors need `code-lsp`, and a smaller interpreter-only `code` Runtime is
# published as its own release asset; both are separate downloads, not handled
# by this script (see the release page).
#
# Env vars:
#   CODE_VERSION  pin to a specific release tag (e.g. v0.3.0) instead of latest
#   PREFIX        install root (default: $HOME/.local); binaries go in $PREFIX/bin
set -eu

REPO="codelovesme/code"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"

need() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "error: '$1' is required but not found on PATH." >&2
        exit 1
    fi
}
need curl
need tar

# --- Platform check -----------------------------------------------------
# Prebuilt binaries are Linux x86_64 only for now (see docs/tickets/high/14-install-script.md).
os="$(uname -s)"
arch="$(uname -m)"
if [ "$os" != "Linux" ] || [ "$arch" != "x86_64" ]; then
    echo "error: prebuilt binaries are only available for Linux x86_64 (detected: $os $arch)." >&2
    echo "Build from source instead — see: https://github.com/$REPO#building" >&2
    exit 1
fi

# --- Resolve version ------------------------------------------------------
if [ -n "${CODE_VERSION:-}" ]; then
    tag="$CODE_VERSION"
else
    echo "Fetching latest release info..."
    tag=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
        | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')
    if [ -z "$tag" ]; then
        echo "error: could not determine the latest release version." >&2
        echo "If a release hasn't been published yet, build from source instead." >&2
        exit 1
    fi
fi

# --- Download + extract ---------------------------------------------------
asset="code-sdk-${tag}-x86_64-linux.tar.gz"
url="https://github.com/$REPO/releases/download/$tag/$asset"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Downloading $url..."
curl -fsSL "$url" -o "$tmp/$asset"

tar -xzf "$tmp/$asset" -C "$tmp"
stage_dir=$(find "$tmp" -maxdepth 1 -type d -name 'code-sdk-*')
if [ -z "$stage_dir" ]; then
    echo "error: unexpected archive layout — no code-sdk-* directory found." >&2
    exit 1
fi

mkdir -p "$BIN_DIR"
cp "$stage_dir/code" "$BIN_DIR/"
chmod +x "$BIN_DIR/code"

echo ""
echo "Installed to $BIN_DIR:"
"$BIN_DIR/code" --version

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
        echo ""
        echo "Note: $BIN_DIR is not on your PATH. Add this to your shell profile:"
        echo "  export PATH=\"$BIN_DIR:\$PATH\""
        ;;
esac

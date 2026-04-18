#!/bin/bash
# Build native N-API addon for the TypeScript SDK.
#
# Compiles the `kimberlite-node` Rust crate via cargo and copies the resulting
# dynamic library into `./native/` with a napi-rs-style platform suffix
# (e.g. kimberlite-node.darwin-arm64.node). The `native/index.js` loader picks
# the right binary at runtime based on process.platform + process.arch.

set -euo pipefail

OS=$(uname -s)
ARCH_UNAME=$(uname -m)

# Normalise arch
case "$ARCH_UNAME" in
    x86_64)  ARCH="x64" ;;
    aarch64|arm64) ARCH="arm64" ;;
    *) echo "Unsupported arch: $ARCH_UNAME" >&2; exit 1 ;;
esac

# Normalise platform + pick dylib extension
case "$OS" in
    Darwin)
        PLATFORM="darwin"
        TRIPLE="${PLATFORM}-${ARCH}"
        DYLIB="libkimberlite_node.dylib"
        ;;
    Linux)
        PLATFORM="linux"
        # Default to gnu libc. CI matrix supplies an explicit TRIPLE override
        # (e.g. TRIPLE=linux-arm64-musl) for non-glibc builds.
        TRIPLE="${TRIPLE:-${PLATFORM}-${ARCH}-gnu}"
        DYLIB="libkimberlite_node.so"
        ;;
    MINGW*|MSYS*|CYGWIN*)
        PLATFORM="win32"
        TRIPLE="${PLATFORM}-${ARCH}-msvc"
        DYLIB="kimberlite_node.dll"
        ;;
    *)
        echo "Unsupported OS: $OS" >&2
        exit 1
        ;;
esac

echo "Building kimberlite-node for ${TRIPLE}..."

# Script runs from sdks/typescript/ — repo root is two levels up.
SDK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
REPO_ROOT="$(cd "$SDK_DIR/../.." && pwd)"

cd "$REPO_ROOT"
cargo build --release -p kimberlite-node
cd "$SDK_DIR"

mkdir -p native

DEST="native/kimberlite-node.${TRIPLE}.node"
cp "$REPO_ROOT/target/release/${DYLIB}" "$DEST"

echo "✓ Native addon at $DEST"
ls -lh native/

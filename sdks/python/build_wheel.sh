#!/bin/bash
# Build Python wheel with bundled native library
# Supports Linux (x86_64, aarch64), macOS (x86_64, arm64), Windows (x86_64)

set -e

# Detect platform
OS=$(uname -s)
ARCH=$(uname -m)

echo "Building Kimberlite Python wheel for $OS $ARCH"

# Build FFI library in release mode
echo "Building FFI library..."
cd ../../
cargo build --release -p kimberlite-ffi
cd sdks/python

# Create lib directory
mkdir -p kimberlite/lib

# Copy native library based on platform
if [[ "$OS" == "Darwin" ]]; then
    echo "Copying macOS library..."
    cp ../../target/release/libkimberlite_ffi.dylib kimberlite/lib/
elif [[ "$OS" == "Linux" ]]; then
    echo "Copying Linux library..."
    cp ../../target/release/libkimberlite_ffi.so kimberlite/lib/
elif [[ "$OS" == "MINGW"* ]] || [[ "$OS" == "MSYS"* ]]; then
    echo "Copying Windows library..."
    cp ../../target/release/kimberlite_ffi.dll kimberlite/lib/
else
    echo "Unsupported platform: $OS"
    exit 1
fi

# Build wheel
echo "Building Python wheel..."
python -m build --wheel

echo "✓ Wheel built successfully"
echo "Output: dist/*.whl"
ls -lh dist/*.whl 2>/dev/null || echo "No wheel files found in dist/"

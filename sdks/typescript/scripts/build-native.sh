#!/bin/bash
# Build native library for TypeScript SDK
# Supports Linux (x86_64, aarch64), macOS (x86_64, arm64), Windows (x86_64)

set -e

# Detect platform
OS=$(uname -s)
ARCH=$(uname -m)

echo "Building native library for TypeScript SDK on $OS $ARCH"

# Build FFI library in release mode
echo "Building FFI library..."
cd ../../..
cargo build --release -p kimberlite-ffi
cd sdks/typescript

# Create native directory
mkdir -p native

# Copy native library based on platform
if [[ "$OS" == "Darwin" ]]; then
    echo "Copying macOS library..."
    cp ../../target/release/libkimberlite_ffi.dylib native/
elif [[ "$OS" == "Linux" ]]; then
    echo "Copying Linux library..."
    cp ../../target/release/libkimberlite_ffi.so native/
elif [[ "$OS" == "MINGW"* ]] || [[ "$OS" == "MSYS"* ]]; then
    echo "Copying Windows library..."
    cp ../../target/release/kimberlite_ffi.dll native/
else
    echo "Unsupported platform: $OS"
    exit 1
fi

echo "✓ Native library copied to native/"
ls -lh native/

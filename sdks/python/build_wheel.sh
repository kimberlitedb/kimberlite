#!/bin/bash
# Build Python wheel with bundled native library.
# Supports Linux (x86_64, aarch64), macOS (x86_64, arm64), Windows (x86_64).
#
# The wheel gets a platform-specific tag via `wheel tags` so PyPI accepts
# it as OS-specific (bundles `libkimberlite_ffi.{so,dylib,dll}`). Each
# OS in the CI matrix emits a uniquely-named wheel — no artifact-name
# collision at publish time.

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

# Copy native library + compute the wheel platform tag.
#
# Platform tags match PEP 600 (manylinux) + PEP 425 conventions. Keep
# the minor versions conservative — `manylinux_2_17` covers glibc ≥ 2.17
# (CentOS 7 / RHEL 7 vintage) which is the current manylinux baseline;
# `macosx_11_0` covers macOS ≥ 11 (Big Sur) for both arm64 and x86_64.
if [[ "$OS" == "Darwin" ]]; then
    echo "Copying macOS library..."
    cp ../../target/release/libkimberlite_ffi.dylib kimberlite/lib/
    if [[ "$ARCH" == "arm64" ]]; then
        PLAT_TAG="macosx_11_0_arm64"
    else
        PLAT_TAG="macosx_11_0_x86_64"
    fi
elif [[ "$OS" == "Linux" ]]; then
    echo "Copying Linux library..."
    cp ../../target/release/libkimberlite_ffi.so kimberlite/lib/
    if [[ "$ARCH" == "aarch64" ]]; then
        PLAT_TAG="manylinux_2_17_aarch64"
    else
        PLAT_TAG="manylinux_2_17_x86_64"
    fi
elif [[ "$OS" == "MINGW"* ]] || [[ "$OS" == "MSYS"* ]]; then
    echo "Copying Windows library..."
    cp ../../target/release/kimberlite_ffi.dll kimberlite/lib/
    PLAT_TAG="win_amd64"
else
    echo "Unsupported platform: $OS"
    exit 1
fi

# Build wheel (setuptools emits `py3-none-any` by default because the
# package has no ext_modules — pyproject.toml declares pure Python).
echo "Building Python wheel..."
python -m build --wheel

# Ensure wheel is installed for the `wheel tags` CLI.
python -m pip install --quiet wheel

# Re-tag the wheel with the platform-specific tag so PyPI accepts it
# as OS-specific. --remove deletes the original py3-none-any wheel so
# only the tagged one ships in the artifact; each OS in the matrix
# therefore emits a uniquely-named wheel and `actions/download-artifact`
# with `merge-multiple: true` no longer collides them.
echo "Re-tagging wheel for platform: $PLAT_TAG"
ORIG_WHEEL=$(ls dist/*.whl | head -1)
python -m wheel tags --platform-tag="$PLAT_TAG" --remove "$ORIG_WHEEL"

echo "✓ Wheel built successfully"
echo "Output:"
ls -lh dist/*.whl 2>/dev/null || echo "No wheel files found in dist/"

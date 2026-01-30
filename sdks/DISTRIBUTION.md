# SDK Distribution Guide

This document describes how to build and publish the Kimberlite Python and TypeScript SDKs.

## Prerequisites

- Rust 1.85+ with cargo
- Python 3.8+ (for Python SDK)
- Node.js 18+ (for TypeScript SDK)
- Platform-specific build tools

### Platform Requirements

**Linux:**
- GCC or Clang
- Make

**macOS:**
- Xcode Command Line Tools
- `xcode-select --install`

**Windows:**
- Visual Studio 2019+ with C++ build tools
- Or MinGW-w64

## Python SDK Distribution

### Local Development Build

```bash
# Build FFI library
cargo build --release -p kimberlite-ffi

# Install SDK in development mode
cd sdks/python
pip install -e .

# Run tests
pytest tests/
```

### Build Wheel for Distribution

```bash
cd sdks/python

# Install build dependencies
pip install build wheel

# Build wheel (includes native library)
./build_wheel.sh

# Output: dist/kimberlite-0.1.0-*.whl
```

### Platform-Specific Wheels

The `build_wheel.sh` script automatically detects your platform and bundles the correct native library:

- **Linux**: `libkimberlite_ffi.so`
- **macOS**: `libkimberlite_ffi.dylib`
- **Windows**: `kimberlite_ffi.dll`

For cross-compilation, use the GitHub Actions workflow which builds wheels for:
- Linux x86_64
- Linux aarch64 (ARM64)
- macOS x86_64 (Intel)
- macOS arm64 (Apple Silicon)
- Windows x86_64

### Publishing to PyPI

```bash
# Install twine
pip install twine

# Check distribution
twine check dist/*

# Upload to PyPI (requires API token)
twine upload dist/*
```

**Environment variables:**
- `TWINE_USERNAME`: Set to `__token__`
- `TWINE_PASSWORD`: Your PyPI API token

### GitHub Actions

The Python SDK workflow (`.github/workflows/sdk-python.yml`) automatically:
1. Runs tests on Linux
2. Builds wheels for Linux, macOS, Windows
3. Uploads wheels as artifacts
4. (Optionally) Publishes to PyPI on main branch

## TypeScript SDK Distribution

### Local Development Build

```bash
# Build FFI library
cargo build --release -p kimberlite-ffi

# Install dependencies and build
cd sdks/typescript
npm install
npm run build

# Run tests
npm test
```

### Build Package for Distribution

```bash
cd sdks/typescript

# Build native library
npm run build:native

# Build TypeScript
npm run build

# Create package
npm pack

# Output: kimberlite-client-0.1.0.tgz
```

### Platform-Specific Packages

The `build-native.sh` script bundles the correct native library:

- **Linux**: `libkimberlite_ffi.so`
- **macOS**: `libkimberlite_ffi.dylib`
- **Windows**: `kimberlite_ffi.dll`

The `package.json` includes the `native/` directory in `files` to ensure it's bundled.

### Publishing to npm

```bash
# Login to npm
npm login

# Publish package (requires npm account)
cd sdks/typescript
npm publish --access public
```

**Authentication:**
- Login interactively: `npm login`
- Or set `NPM_TOKEN` environment variable

### GitHub Actions

The TypeScript SDK workflow (`.github/workflows/sdk-typescript.yml`) automatically:
1. Runs type checking and tests
2. Builds packages for Linux, macOS, Windows
3. Uploads packages as artifacts
4. (Optionally) Publishes to npm on main branch

## Multi-Platform Build Strategy

### Option 1: GitHub Actions (Recommended)

Use the provided GitHub Actions workflows to build on all platforms automatically:

```bash
# Trigger workflow manually
gh workflow run sdk-python.yml
gh workflow run sdk-typescript.yml
```

Artifacts will be uploaded and can be downloaded from the Actions tab.

### Option 2: Docker (Linux only)

Build Linux wheels in Docker:

```bash
# Build in manylinux container
docker run --rm -v $(pwd):/workspace \
  quay.io/pypa/manylinux2014_x86_64 \
  /workspace/sdks/python/build_wheel.sh
```

### Option 3: Manual Cross-Platform

Build on each platform separately:

1. **Linux**: Build on Ubuntu/Debian machine
2. **macOS**: Build on macOS machine (Intel or Apple Silicon)
3. **Windows**: Build on Windows machine or use WSL

## Release Checklist

### Python SDK

- [ ] Update version in `sdks/python/pyproject.toml`
- [ ] Update CHANGELOG.md
- [ ] Run tests: `pytest sdks/python/tests/`
- [ ] Type check: `mypy sdks/python/kimberlite --strict`
- [ ] Build wheels for all platforms
- [ ] Test wheels on each platform
- [ ] Create Git tag: `git tag python-v0.1.0`
- [ ] Push tag: `git push origin python-v0.1.0`
- [ ] Upload to PyPI: `twine upload dist/*`

### TypeScript SDK

- [ ] Update version in `sdks/typescript/package.json`
- [ ] Update CHANGELOG.md
- [ ] Run tests: `npm test`
- [ ] Type check: `npm run type-check`
- [ ] Build packages for all platforms
- [ ] Test packages on each platform
- [ ] Create Git tag: `git tag typescript-v0.1.0`
- [ ] Push tag: `git push origin typescript-v0.1.0`
- [ ] Publish to npm: `npm publish --access public`

## Version Management

Both SDKs follow semantic versioning (SemVer):

- **MAJOR**: Breaking API changes
- **MINOR**: New features, backward compatible
- **PATCH**: Bug fixes, backward compatible

Example: `0.1.0` → `0.2.0` (new feature) → `0.2.1` (bug fix)

## Troubleshooting

### Python: "Cannot find libkimberlite_ffi"

Ensure the FFI library was built and copied to `kimberlite/lib/`:

```bash
cargo build --release -p kimberlite-ffi
mkdir -p sdks/python/kimberlite/lib
cp target/release/libkimberlite_ffi.* sdks/python/kimberlite/lib/
```

### TypeScript: "Cannot find native module"

Ensure the FFI library was copied to `native/`:

```bash
cargo build --release -p kimberlite-ffi
mkdir -p sdks/typescript/native
cp target/release/libkimberlite_ffi.* sdks/typescript/native/
```

### Cross-Platform Compatibility

If wheels/packages built on one platform don't work on another:

1. Verify the correct native library is bundled
2. Check architecture matches (x86_64 vs ARM64)
3. Ensure dynamic library dependencies are met

## Security Considerations

### PyPI/npm Token Management

- **Never commit tokens** to version control
- Store tokens in GitHub Secrets for CI/CD
- Use scoped tokens with minimal permissions
- Rotate tokens regularly

### Native Library Verification

Before publishing, verify the native libraries:

```bash
# Check library dependencies (Linux)
ldd sdks/python/kimberlite/lib/libkimberlite_ffi.so

# Check library dependencies (macOS)
otool -L sdks/python/kimberlite/lib/libkimberlite_ffi.dylib

# Check library dependencies (Windows)
dumpbin /dependents sdks/python/kimberlite/lib/kimberlite_ffi.dll
```

## Support

For distribution issues:
- Python SDK: Open issue with `sdk:python` label
- TypeScript SDK: Open issue with `sdk:typescript` label
- Build system: Open issue with `build` label

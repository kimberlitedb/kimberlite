---
title: "Installation"
section: "start"
slug: "installation"
order: 2
---

# Installation

Install Kimberlite on your platform.

## Prerequisites

### Required

- **Rust 1.88+** - Install from [rustup.rs](https://rustup.rs)
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```

- **Git** - For cloning the repository
  ```bash
  # macOS
  brew install git

  # Ubuntu/Debian
  sudo apt install git

  # Windows
  # Download from https://git-scm.com
  ```

### Optional (Recommended)

- **just** - Command runner (like Make but better)
  ```bash
  cargo install just
  ```

- **cargo-nextest** - Faster test runner
  ```bash
  cargo install cargo-nextest
  ```

- **cargo-watch** or **bacon** - Watch mode for development
  ```bash
  cargo install cargo-watch
  # or
  cargo install bacon
  ```

## Installation Options

### Option 1: Build from Source (Current)

This is the only option available in v0.4.0 since Kimberlite is still in early development.

```bash
# Clone the repository
git clone https://github.com/kimberlitedb/kimberlite.git
cd kimberlite

# Verify Rust version
rustc --version
# Should be 1.88 or later

# Build the workspace
cargo build --workspace

# Run tests to verify installation
cargo test --workspace
```

**Build time:** ~5-10 minutes on first build (subsequent builds are incremental).

### Option 2: Install from crates.io (Coming v0.5.0)

Not yet available. Once released:

```bash
# Install CLI tools
cargo install kimberlite-cli

# Verify installation
vopr --version
```

### Option 3: Docker (Coming v0.5.0)

Not yet available. Once released:

```bash
# Pull the image
docker pull kimberlite/kimberlite:latest

# Run a container
docker run -p 5432:5432 kimberlite/kimberlite
```

### Option 4: Pre-built Binaries (Coming v0.6.0)

Not yet available. Once released, download from GitHub Releases:

```bash
# macOS (Homebrew)
brew install kimberlite

# Ubuntu/Debian
curl -sL https://github.com/kimberlitedb/kimberlite/releases/download/v0.6.0/kimberlite-linux-amd64.deb
sudo dpkg -i kimberlite-linux-amd64.deb

# Windows (Scoop)
scoop install kimberlite
```

## Verify Installation

After building from source:

```bash
# Check that libraries build
cargo build --workspace

# Run the test suite
cargo test --workspace

# Run VOPR (simulation testing tool)
cargo run --bin vopr -- --version
```

You should see:
```
vopr 0.4.0
```

## Platform-Specific Notes

### macOS

**Apple Silicon (M1/M2/M3):**
- ✅ Full support, native ARM64 builds
- TLS uses `aws-lc-rs` instead of `ring` (better compatibility)

**Intel (x86_64):**
- ✅ Full support

### Linux

**Ubuntu/Debian:**
```bash
# Install build dependencies
sudo apt update
sudo apt install build-essential pkg-config libssl-dev
```

**RHEL/CentOS/Fedora:**
```bash
sudo dnf groupinstall "Development Tools"
sudo dnf install openssl-devel
```

### Windows

**Using WSL2 (Recommended):**
1. Install WSL2: `wsl --install`
2. Install Ubuntu from Microsoft Store
3. Follow Linux installation steps above

**Native Windows:**
- ✅ Builds with MSVC toolchain
- Requires Visual Studio Build Tools

## Development Setup

### 1. Install Justfile Runner

```bash
cargo install just
```

Then you can use convenient commands:
```bash
just build         # Build project
just test          # Run tests
just pre-commit    # Pre-commit checks
just vopr          # Run VOPR testing
```

See all commands: `just --list`

### 2. Install Nextest (Optional)

Faster test runner:
```bash
cargo install cargo-nextest
```

Then use:
```bash
just nextest      # Run tests with nextest
```

### 3. Set Up Watch Mode (Optional)

For live reloading during development:

**bacon** (recommended):
```bash
cargo install bacon
bacon             # Start watching
```

**cargo-watch**:
```bash
cargo install cargo-watch
cargo watch -x test
```

### 4. IDE Setup

**VS Code:**
1. Install `rust-analyzer` extension
2. Install `Even Better TOML` extension
3. Optional: Install `CodeLLDB` for debugging

**IntelliJ IDEA / CLion:**
1. Install Rust plugin
2. Open project (Cargo.toml will be detected)

**Neovim/Vim:**
1. Install `rust-analyzer` LSP
2. Install `vim-rust` or similar plugin

## Updating

### Update from Source

```bash
cd kimberlite
git pull origin main
cargo build --workspace
```

### Update from crates.io (Once Available)

```bash
cargo install kimberlite-cli --force
```

## Uninstall

### Remove Source Build

```bash
cd kimberlite
cargo clean
cd ..
rm -rf kimberlite
```

### Remove crates.io Install (Once Available)

```bash
cargo uninstall kimberlite-cli
```

## Troubleshooting

### Build Fails: "error: linker `cc` not found"

**Solution (Linux):**
```bash
sudo apt install build-essential
```

**Solution (macOS):**
```bash
xcode-select --install
```

### Build Fails: "failed to run custom build command for `openssl-sys`"

**Solution:**
```bash
# macOS
brew install openssl
export OPENSSL_DIR=$(brew --prefix openssl)

# Linux
sudo apt install libssl-dev pkg-config
```

### Build Fails on Apple Silicon: "ring" Compilation Error

This shouldn't happen (we use `aws-lc-rs`), but if it does:

**Solution:**
Ensure you're on the latest version that uses `aws-lc-rs` instead of `ring`.

### Tests Fail with "Too many open files"

**Solution (macOS/Linux):**
```bash
# Increase file descriptor limit
ulimit -n 4096
```

Add to `~/.bashrc` or `~/.zshrc` to make permanent:
```bash
ulimit -n 4096
```

### Slow Build Times

**Solutions:**
1. Use release profile for faster binaries: `cargo build --release`
2. Enable parallel compilation: `export CARGO_BUILD_JOBS=8`
3. Use `sccache` for build caching: `cargo install sccache`

### "command not found: cargo"

**Solution:**
Ensure Rust is in your PATH:
```bash
source $HOME/.cargo/env
```

Add to `~/.bashrc` or `~/.zshrc`:
```bash
source $HOME/.cargo/env
```

## Next Steps

After installation:

1. **[Quick Start](quick-start.md)** - Get running in 10 minutes
2. **[First Application](first-app.md)** - Build a simple healthcare app
3. **[CLAUDE.md](../../CLAUDE.md)** - Full development guide
4. **[Contributing](../../docs-internal/contributing/getting-started.md)** - Contribute to Kimberlite

## System Requirements

### Minimum

- **CPU:** 2 cores
- **RAM:** 4 GB
- **Disk:** 2 GB for source + build artifacts
- **OS:** macOS 10.15+, Linux (kernel 4.4+), Windows 10+

### Recommended

- **CPU:** 4+ cores (faster compilation)
- **RAM:** 8+ GB
- **Disk:** 10 GB (for development with multiple builds)
- **SSD:** Recommended for fast compilation

## Getting Help

- **Installation Issues:** Open an issue on [GitHub](https://github.com/kimberlitedb/kimberlite/issues)
- **Build Troubleshooting:** See [Troubleshooting Guide](../operating/troubleshooting.md)
- **General Questions:** Ask in [GitHub Discussions](https://github.com/kimberlitedb/kimberlite/discussions)

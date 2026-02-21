---
title: "Installation"
section: "start"
slug: "installation"
order: 1
---

# Installation

Kimberlite ships as a single binary: `kimberlite`. Install it for your platform using one of the methods below.

## Install Script (Recommended)

The quickest way to install on macOS or Linux:

```bash
curl -fsSL https://kimberlite.dev/install.sh | sh
```

This downloads the latest release, verifies its checksum, and installs `kimberlite` to `/usr/local/bin`. A shorter alias `kmb` is also installed as a convenience shortcut.

## macOS

### Homebrew

```bash
brew install kimberlitedb/tap/kimberlite
```

### Direct Download

```bash
# Apple Silicon (M1/M2/M3)
curl -fsSL https://github.com/kimberlite/kimberlite/releases/latest/download/kimberlite-macos-aarch64.zip -o kimberlite.zip
unzip kimberlite.zip && chmod +x kimberlite && sudo mv kimberlite /usr/local/bin/

# Intel Mac
curl -fsSL https://github.com/kimberlite/kimberlite/releases/latest/download/kimberlite-macos-x86_64.zip -o kimberlite.zip
unzip kimberlite.zip && chmod +x kimberlite && sudo mv kimberlite /usr/local/bin/
```

## Linux

```bash
# x86_64
curl -fsSL https://github.com/kimberlite/kimberlite/releases/latest/download/kimberlite-linux-x86_64.zip -o kimberlite.zip
unzip kimberlite.zip && chmod +x kimberlite && sudo mv kimberlite /usr/local/bin/

# ARM64
curl -fsSL https://github.com/kimberlite/kimberlite/releases/latest/download/kimberlite-linux-aarch64.zip -o kimberlite.zip
unzip kimberlite.zip && chmod +x kimberlite && sudo mv kimberlite /usr/local/bin/
```

## Docker

```bash
docker pull ghcr.io/kimberlitedb/kimberlite:latest
docker run --rm -it ghcr.io/kimberlitedb/kimberlite:latest --help
```

## Build from Source

Requires Rust 1.88+.

```bash
git clone https://github.com/kimberlite/kimberlite.git
cd kimberlite
cargo build --release -p kimberlite-cli
sudo cp target/release/kimberlite /usr/local/bin/
```

## Verify Installation

```bash
kimberlite --version
```

Expected output:
```
kimberlite 0.4.0
```

## Next Steps

- **[Quick Start](quick-start.md)** — Run your first queries in 5 minutes

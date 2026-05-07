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

This detects your OS and architecture automatically, downloads the latest release, **verifies its SHA-256 checksum against the release's `SHA256SUMS` manifest**, and installs `kimberlite` to `/usr/local/bin` (or `~/.kimberlite/bin` if that's not writable). A shorter alias `kmb` is installed alongside as a symlink. To pin a specific version: `curl -fsSL https://kimberlite.dev/install.sh | sh -s -- --version v0.8.0`. To bypass checksum verification (not recommended), set `KIMBERLITE_SKIP_CHECKSUM=1`.

## macOS

### Homebrew

```bash
brew install kimberlitedb/tap/kimberlite
```

### Direct Download

```bash
# Apple Silicon (M1/M2/M3/M4)
curl -fsSL https://mac.kimberlite.dev -o kimberlite.zip
unzip kimberlite.zip && chmod +x kimberlite && sudo mv kimberlite /usr/local/bin/

# Intel Mac
curl -fsSL https://mac-intel.kimberlite.dev -o kimberlite.zip
unzip kimberlite.zip && chmod +x kimberlite && sudo mv kimberlite /usr/local/bin/
```

## Linux

```bash
# x86_64
curl -fsSL https://linux.kimberlite.dev -o kimberlite.zip
unzip kimberlite.zip && chmod +x kimberlite && sudo mv kimberlite /usr/local/bin/

# ARM64
curl -fsSL https://linux-arm.kimberlite.dev -o kimberlite.zip
unzip kimberlite.zip && chmod +x kimberlite && sudo mv kimberlite /usr/local/bin/
```

## Windows

Download from the [download page](https://kimberlite.dev/download) and extract the zip. Add the directory to your PATH, then verify:

```powershell
kimberlite.exe version
```

## Docker

```bash
docker pull ghcr.io/kimberlitedb/kimberlite:latest
docker run --rm -it ghcr.io/kimberlitedb/kimberlite:latest --help
```

## Build from Source

Requires Rust 1.88+.

```bash
git clone https://github.com/kimberlitedb/kimberlite.git
cd kimberlite
cargo build --release --bin kimberlite
sudo cp target/release/kimberlite /usr/local/bin/
```

## Verify Installation

```bash
kimberlite version
```

Expected output:
```
kimberlite 0.4.0
```

## Next Steps

- **[Quick Start](quick-start.md)** — Run your first queries in 5 minutes

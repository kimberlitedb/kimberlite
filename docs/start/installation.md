---
title: "Installation"
section: "start"
slug: "installation"
order: 1
---

# Installation

Kimberlite ships as a single binary: `kmb`. Install it for your platform using one of the methods below.

## macOS

### Homebrew (Recommended)

```bash
brew install kimberlite/tap/kmb
```

### Direct Download

```bash
curl -fsSL https://releases.kimberlite.dev/latest/kmb-macos-arm64 -o /usr/local/bin/kmb
chmod +x /usr/local/bin/kmb
```

## Linux

```bash
curl -fsSL https://releases.kimberlite.dev/latest/kmb-linux-x86_64 -o /usr/local/bin/kmb
chmod +x /usr/local/bin/kmb
```

## Docker

```bash
docker pull ghcr.io/kimberlite/kmb:latest
docker run --rm -it ghcr.io/kimberlite/kmb:latest --help
```

## Build from Source

Requires Rust 1.88+.

```bash
git clone https://github.com/kimberlite/kimberlite.git
cd kimberlite
cargo build --release --bin kmb
cp target/release/kmb /usr/local/bin/
```

## Verify Installation

```bash
kmb --version
```

Expected output:
```
kmb 0.4.0
```

## Next Steps

- **[Quick Start](quick-start.md)** — Run your first queries in 5 minutes

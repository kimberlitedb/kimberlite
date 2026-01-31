# Kimberlite Download URLs

This document tracks all download URLs for Kimberlite binaries.

## Short URLs (Cloudflare Redirects)

These are the user-facing, easy-to-remember URLs that redirect to GitHub releases.

### Default Platforms (Most Common)

| URL | Points To | Platform | Notes |
|-----|-----------|----------|-------|
| `https://linux.kimberlite.dev` | `kimberlite-linux-x86_64.zip` | Linux x86_64 | Most common server architecture |
| `https://mac.kimberlite.dev` | `kimberlite-macos-aarch64.zip` | macOS ARM64 | Default to Apple Silicon (M1/M2/M3/M4) |
| `https://windows.kimberlite.dev` | `kimberlite-windows-x86_64.zip` | Windows x86_64 | Uses ctrlc for signal handling |

### Specific Architectures

| URL | Points To | Platform |
|-----|-----------|----------|
| `https://linux-x86.kimberlite.dev` | `kimberlite-linux-x86_64.zip` | Linux x86_64 |
| `https://linux-arm.kimberlite.dev` | `kimberlite-linux-aarch64.zip` | Linux ARM64 |
| `https://mac-arm.kimberlite.dev` | `kimberlite-macos-aarch64.zip` | macOS Apple Silicon |
| `https://mac-intel.kimberlite.dev` | `kimberlite-macos-x86_64.zip` | macOS Intel |
| `https://windows-x86.kimberlite.dev` | `kimberlite-windows-x86_64.zip` | Windows x86_64 |

## Direct GitHub URLs

These are the actual download URLs on GitHub releases.

### Latest Release (Auto-updates)

```
https://github.com/kimberlitedb/kimberlite/releases/latest/download/kimberlite-linux-x86_64.zip
https://github.com/kimberlitedb/kimberlite/releases/latest/download/kimberlite-linux-aarch64.zip
https://github.com/kimberlitedb/kimberlite/releases/latest/download/kimberlite-macos-aarch64.zip
https://github.com/kimberlitedb/kimberlite/releases/latest/download/kimberlite-macos-x86_64.zip
https://github.com/kimberlitedb/kimberlite/releases/latest/download/kimberlite-windows-x86_64.zip
```

### Specific Version (Example: v0.1.0)

```
https://github.com/kimberlitedb/kimberlite/releases/download/v0.1.0/kimberlite-linux-x86_64.zip
https://github.com/kimberlitedb/kimberlite/releases/download/v0.1.0/kimberlite-linux-aarch64.zip
https://github.com/kimberlitedb/kimberlite/releases/download/v0.1.0/kimberlite-macos-aarch64.zip
https://github.com/kimberlitedb/kimberlite/releases/download/v0.1.0/kimberlite-macos-x86_64.zip
https://github.com/kimberlitedb/kimberlite/releases/download/v0.1.0/kimberlite-windows-x86_64.zip
```

## Checksums

Always available alongside releases:

```
https://github.com/kimberlitedb/kimberlite/releases/latest/download/checksums.txt
```

## Installation Commands

### Linux (Default: x86_64)

```bash
curl -Lo kimberlite.zip https://linux.kimberlite.dev
unzip kimberlite.zip
./kimberlite --version
```

### macOS (Default: Apple Silicon)

```bash
curl -Lo kimberlite.zip https://mac.kimberlite.dev
unzip kimberlite.zip
./kimberlite --version
```

### Windows (Default: x86_64)

**PowerShell:**
```powershell
curl -Lo kimberlite.zip https://windows.kimberlite.dev
Expand-Archive kimberlite.zip
.\kimberlite\kimberlite.exe --version
```

**Command Prompt:**
```cmd
curl -Lo kimberlite.zip https://windows.kimberlite.dev
tar -xf kimberlite.zip
kimberlite.exe --version
```

### Verify Checksum

```bash
# Download checksums
curl -Lo checksums.txt https://github.com/kimberlitedb/kimberlite/releases/latest/download/checksums.txt

# Verify (Linux/macOS)
sha256sum -c checksums.txt --ignore-missing
```

## Website Usage

### Homepage (home.html)

- **Linux tab**: Uses `https://linux.kimberlite.dev` (default x86_64)
- **macOS tab**: Uses `https://mac.kimberlite.dev` (default Apple Silicon)
- **Windows tab**: Uses `https://windows.kimberlite.dev` (not yet available - shows warning)

### Download Page (download.html)

**Quick Downloads (defaults):**
- Linux: `https://linux.kimberlite.dev`
- macOS: `https://mac.kimberlite.dev`
- Windows: Disabled with "Coming Soon" badge

**All Platforms (direct links):**
- Linux x86_64: Direct GitHub URL
- Linux ARM64: Direct GitHub URL
- macOS Apple Silicon: Direct GitHub URL
- macOS Intel: Direct GitHub URL

## Cloudflare Configuration

All short URLs use Cloudflare redirect rules:

- **Type**: Static redirect
- **Status code**: 302 (or 301 for permanent)
- **Preserve query string**: Optional (not needed for downloads)
- **DNS**: A record pointing to dummy IP (192.0.2.1), proxied (orange cloud)

## Platform Support Status

| Platform | Status | Binary Size | Notes |
|----------|--------|-------------|-------|
| Linux x86_64 | ✅ Available | ~6MB | Tested in CI |
| Linux ARM64 | ✅ Available | ~6MB | Cross-compiled, CI verified |
| macOS ARM64 | ✅ Available | 5.9MB | Native build, tested |
| macOS x86_64 | ✅ Available | ~6MB | Zigbuild cross-compile |
| Windows x86_64 | ✅ Available | ~6MB | Uses `ctrlc` for Ctrl+C handling |
| Windows ARM64 | ⏳ Planned | - | Future support |

## Windows Support Implementation

**Solution**: Cross-platform signal handling using conditional compilation.

- **Unix** (Linux, macOS): Uses `signal-hook` and `signal-hook-mio` for SIGTERM/SIGINT
- **Windows**: Uses `ctrlc` crate for Ctrl+C and Ctrl+Break handling

**Implementation**: `crates/kmb-server/src/server.rs` with `#[cfg(unix)]` and `#[cfg(windows)]` directives.

## Marketing Copy

### For Documentation

> Download Kimberlite for your platform. Pre-built binaries available for Linux and macOS. Windows support coming soon.

### For Release Notes

> **Downloads:**
> - **Linux**: `curl -Lo kimberlite.zip https://linux.kimberlite.dev`
> - **macOS**: `curl -Lo kimberlite.zip https://mac.kimberlite.dev`
> - Or download directly from the [releases page](https://github.com/kimberlitedb/kimberlite/releases)

## Future Enhancements

1. **Windows Support**: Implement cross-platform signal handling
2. **Homebrew**: `brew install kimberlite`
3. **APT/YUM**: Linux package repositories
4. **Docker**: Official container images
5. **Nix**: Nix package
6. **Snap/Flatpak**: Universal Linux packages
7. **Checksums on Website**: Show SHA-256 directly on download page

## Related Files

- `.github/workflows/release.yml` - Binary build configuration
- `website/templates/home.html` - Homepage installation section
- `website/templates/download.html` - Dedicated download page
- `BUILD_VERIFICATION.md` - Cross-platform build verification details

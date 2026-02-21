#!/usr/bin/env sh
# Kimberlite installer
# Usage: curl -fsSL https://kimberlite.dev/install.sh | sh

set -eu

REPO="kimberlite/kimberlite"
BINARY="kimberlite"
INSTALL_DIR="${KIMBERLITE_INSTALL_DIR:-/usr/local/bin}"

# Detect OS and architecture
os=""
arch=""

case "$(uname -s)" in
  Darwin) os="macos" ;;
  Linux)  os="linux" ;;
  *)
    echo "Unsupported OS: $(uname -s)"
    echo "Please download manually from https://github.com/${REPO}/releases"
    exit 1
    ;;
esac

case "$(uname -m)" in
  x86_64 | amd64) arch="x86_64" ;;
  aarch64 | arm64) arch="aarch64" ;;
  *)
    echo "Unsupported architecture: $(uname -m)"
    echo "Please download manually from https://github.com/${REPO}/releases"
    exit 1
    ;;
esac

ARTIFACT="${BINARY}-${os}-${arch}"
URL="https://github.com/${REPO}/releases/latest/download/${ARTIFACT}.zip"

echo "Downloading Kimberlite for ${os}/${arch}..."

# Download to temp dir
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

if command -v curl >/dev/null 2>&1; then
  curl -fsSL "${URL}" -o "${TMP_DIR}/${ARTIFACT}.zip"
elif command -v wget >/dev/null 2>&1; then
  wget -q "${URL}" -O "${TMP_DIR}/${ARTIFACT}.zip"
else
  echo "Error: curl or wget is required"
  exit 1
fi

cd "${TMP_DIR}"
unzip -q "${ARTIFACT}.zip"
chmod +x "${BINARY}"

# Install
if [ -w "${INSTALL_DIR}" ]; then
  mv "${BINARY}" "${INSTALL_DIR}/${BINARY}"
else
  echo "Installing to ${INSTALL_DIR} (may prompt for sudo password)..."
  sudo mv "${BINARY}" "${INSTALL_DIR}/${BINARY}"
fi

# Verify
if "${INSTALL_DIR}/${BINARY}" --version >/dev/null 2>&1; then
  VERSION="$("${INSTALL_DIR}/${BINARY}" --version)"
  echo ""
  echo "✓ Installed ${VERSION}"
  echo "  Run: kimberlite --help"
else
  echo ""
  echo "✓ Kimberlite installed to ${INSTALL_DIR}/${BINARY}"
fi

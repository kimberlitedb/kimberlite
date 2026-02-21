#!/bin/sh
# Kimberlite Install Script
# Usage: curl -fsSL https://kimberlite.dev/install.sh | sh
#        curl -fsSL https://kimberlite.dev/install.sh | sh -s -- --version v0.4.0
#
# This script detects your OS and architecture, downloads the correct
# Kimberlite binary from GitHub releases, and installs it.
#
# NOTE: This file is mirrored to website/public/install.sh.
# Keep both files in sync when making changes.

set -eu

REPO="kimberlitedb/kimberlite"
INSTALL_DIR="${KIMBERLITE_INSTALL_DIR:-}"
REQUESTED_VERSION=""

# --- Helpers ---

info() {
    printf "\033[1;34m==>\033[0m %s\n" "$1"
}

success() {
    printf "\033[1;32m==>\033[0m %s\n" "$1"
}

error() {
    printf "\033[1;31merror:\033[0m %s\n" "$1" >&2
    exit 1
}

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1; then
        error "need '$1' (command not found)"
    fi
}

# --- Argument parsing ---

while [ $# -gt 0 ]; do
    case "$1" in
        --version)
            shift
            if [ $# -eq 0 ]; then
                error "--version requires a value (e.g., --version v0.4.0)"
            fi
            REQUESTED_VERSION="$1"
            ;;
        --version=*)
            REQUESTED_VERSION="${1#--version=}"
            ;;
        --help|-h)
            cat <<'USAGE'
Kimberlite Install Script

USAGE:
    curl -fsSL https://kimberlite.dev/install.sh | sh
    curl -fsSL https://kimberlite.dev/install.sh | sh -s -- [OPTIONS]

OPTIONS:
    --version <VERSION>    Install a specific version (e.g., v0.4.0)
    --help, -h             Show this help message

ENVIRONMENT:
    KIMBERLITE_INSTALL_DIR    Override install directory
                              (default: ~/.kimberlite/bin or /usr/local/bin)

EXAMPLES:
    # Install latest version
    curl -fsSL https://kimberlite.dev/install.sh | sh

    # Install specific version
    curl -fsSL https://kimberlite.dev/install.sh | sh -s -- --version v0.4.0
USAGE
            exit 0
            ;;
        *)
            error "unknown option: $1 (try --help)"
            ;;
    esac
    shift
done

# --- Detect platform ---

detect_os() {
    case "$(uname -s)" in
        Linux*)     echo "linux" ;;
        Darwin*)    echo "macos" ;;
        MINGW*|MSYS*|CYGWIN*)
            error "Windows detected. Please download from https://github.com/$REPO/releases or use: winget install kimberlite"
            ;;
        *)          error "unsupported operating system: $(uname -s)" ;;
    esac
}

detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)   echo "x86_64" ;;
        aarch64|arm64)  echo "aarch64" ;;
        *)              error "unsupported architecture: $(uname -m)" ;;
    esac
}

# --- Resolve version ---

resolve_version() {
    if [ -n "$REQUESTED_VERSION" ]; then
        echo "$REQUESTED_VERSION"
        return
    fi

    info "Fetching latest release version..."

    if command -v curl > /dev/null 2>&1; then
        version=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
            | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"//;s/".*//')
    elif command -v wget > /dev/null 2>&1; then
        version=$(wget -qO- "https://api.github.com/repos/$REPO/releases/latest" \
            | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"//;s/".*//')
    else
        error "need 'curl' or 'wget' to download Kimberlite"
    fi

    if [ -z "$version" ]; then
        error "could not determine latest version. Try: --version v0.4.0"
    fi

    echo "$version"
}

# --- Download ---

download() {
    url="$1"
    dest="$2"

    if command -v curl > /dev/null 2>&1; then
        curl -fsSL "$url" -o "$dest"
    elif command -v wget > /dev/null 2>&1; then
        wget -q "$url" -O "$dest"
    else
        error "need 'curl' or 'wget' to download"
    fi
}

# --- Choose install directory ---

choose_install_dir() {
    if [ -n "$INSTALL_DIR" ]; then
        echo "$INSTALL_DIR"
        return
    fi

    # Prefer /usr/local/bin if writable
    if [ -d "/usr/local/bin" ] && [ -w "/usr/local/bin" ]; then
        echo "/usr/local/bin"
        return
    fi

    # Fall back to ~/.kimberlite/bin
    echo "$HOME/.kimberlite/bin"
}

# --- Add to PATH ---

add_to_path() {
    install_dir="$1"

    # Check if already in PATH
    case ":$PATH:" in
        *":$install_dir:"*) return ;;
    esac

    export_line="export PATH=\"$install_dir:\$PATH\""

    # Detect shell and update profile
    added=false
    for profile in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.profile"; do
        if [ -f "$profile" ]; then
            if ! grep -q "kimberlite" "$profile" 2>/dev/null; then
                printf '\n# Kimberlite\n%s\n' "$export_line" >> "$profile"
                info "Added to PATH in $(basename "$profile")"
                added=true
            fi
        fi
    done

    # If no profile found, create .profile
    if [ "$added" = false ]; then
        printf '\n# Kimberlite\n%s\n' "$export_line" >> "$HOME/.profile"
        info "Added to PATH in .profile"
    fi
}

# --- Main ---

main() {
    printf "\n\033[1mKimberlite Installer\033[0m\n\n"

    os=$(detect_os)
    arch=$(detect_arch)
    version=$(resolve_version)
    artifact_name="kimberlite-${os}-${arch}"
    download_url="https://github.com/$REPO/releases/download/$version/${artifact_name}.zip"

    info "Platform: $os/$arch"
    info "Version:  $version"

    # Need unzip
    need_cmd "unzip"

    # Create temp directory
    tmp_dir=$(mktemp -d)
    trap 'rm -rf "$tmp_dir"' EXIT

    # Download
    info "Downloading $artifact_name..."
    download "$download_url" "$tmp_dir/$artifact_name.zip"

    # Extract
    info "Extracting..."
    unzip -q "$tmp_dir/$artifact_name.zip" -d "$tmp_dir/extracted"

    # Find binary
    binary="$tmp_dir/extracted/kimberlite"
    if [ ! -f "$binary" ]; then
        error "binary not found in archive"
    fi
    chmod +x "$binary"

    # Install
    install_dir=$(choose_install_dir)
    mkdir -p "$install_dir"

    info "Installing to $install_dir/kimberlite..."
    cp "$binary" "$install_dir/kimberlite"

    # Symlink as 'kmb' for convenience
    if [ ! -f "$install_dir/kmb" ] || [ -L "$install_dir/kmb" ]; then
        ln -sf "$install_dir/kimberlite" "$install_dir/kmb"
    fi

    # Add to PATH if needed
    add_to_path "$install_dir"

    # Verify
    if "$install_dir/kimberlite" version > /dev/null 2>&1; then
        installed_version=$("$install_dir/kimberlite" version 2>/dev/null | head -1 || echo "unknown")
        success "Kimberlite installed successfully! ($installed_version)"
    else
        success "Kimberlite installed to $install_dir/kimberlite"
    fi

    # Next steps
    printf "\n\033[1mNext steps:\033[0m\n"
    printf "  1. Open a new terminal (or run: source ~/.bashrc)\n"
    printf "  2. Initialize a project:\n"
    printf "     \033[36mkimberlite init my-project\033[0m\n"
    printf "  3. Start the development server:\n"
    printf "     \033[36mcd my-project && kimberlite dev\033[0m\n"
    printf "\n  Documentation: https://kimberlite.dev/docs\n\n"
}

main

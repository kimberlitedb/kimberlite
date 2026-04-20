#!/bin/sh
# Kimberlite Install Script
# Usage: curl -fsSL https://kimberlite.dev/install.sh | sh
#        curl -fsSL https://kimberlite.dev/install.sh | sh -s -- --version v0.4.2
#
# This script detects your OS and architecture, downloads the correct
# Kimberlite binary from GitHub releases, verifies its SHA-256 checksum
# against the release's SHA256SUMS manifest, installs the binary, and
# creates a `kmb` alias symlink.
#
# NOTE: This file is mirrored to website/public/install.sh.
# Keep both files in sync when making changes.

set -eu

REPO="kimberlitedb/kimberlite"
INSTALL_DIR="${KIMBERLITE_INSTALL_DIR:-}"
REQUESTED_VERSION=""
SKIP_CHECKSUM="${KIMBERLITE_SKIP_CHECKSUM:-0}"

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
    --version <VERSION>    Install a specific version (e.g., v0.4.2)
    --help, -h             Show this help message

ENVIRONMENT:
    KIMBERLITE_INSTALL_DIR    Override install directory
                              (default: /usr/local/bin if writable,
                              otherwise ~/.kimberlite/bin)
    KIMBERLITE_SKIP_CHECKSUM  Set to 1 to skip SHA-256 checksum verification
                              (not recommended; default: 0)

EXAMPLES:
    # Install latest version
    curl -fsSL https://kimberlite.dev/install.sh | sh

    # Install specific version
    curl -fsSL https://kimberlite.dev/install.sh | sh -s -- --version v0.4.2
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
        error "could not determine latest version. Try: --version v0.4.2"
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

# --- Checksum verification ---
#
# Underscored local names (_file, _version, _path, _name, …) throughout:
# POSIX sh does not provide function-local variables, so plain `foo=...`
# inside a function would clobber any same-named variable in main(). The
# original bug (fixed here) collided verify_checksum's `artifact_name`
# with main()'s, producing double-`.zip` paths at extract time.

# Computes SHA-256 of $1 using whichever tool is available. Prints the
# lowercase hex digest on stdout, or returns non-zero if no tool is found.
sha256_of() {
    _file="$1"
    if command -v sha256sum > /dev/null 2>&1; then
        sha256sum "$_file" | awk '{print $1}'
    elif command -v shasum > /dev/null 2>&1; then
        shasum -a 256 "$_file" | awk '{print $1}'
    elif command -v openssl > /dev/null 2>&1; then
        openssl dgst -sha256 "$_file" | awk '{print $NF}'
    else
        return 1
    fi
}

# Fetches the SHA256SUMS manifest from the release and verifies the
# artifact's digest matches. Set KIMBERLITE_SKIP_CHECKSUM=1 to opt out
# (not recommended).
verify_checksum() {
    _ver="$1"
    _path="$2"
    _name="$3"

    if [ "$SKIP_CHECKSUM" = "1" ]; then
        info "Checksum verification skipped (KIMBERLITE_SKIP_CHECKSUM=1)"
        return 0
    fi

    info "Verifying SHA-256 checksum..."

    _sums_url="https://github.com/$REPO/releases/download/$_ver/SHA256SUMS"
    _sums_file="$(dirname "$_path")/SHA256SUMS"
    if ! download "$_sums_url" "$_sums_file" 2>/dev/null; then
        error "failed to download SHA256SUMS from $_sums_url. Set KIMBERLITE_SKIP_CHECKSUM=1 to bypass (not recommended)."
    fi

    _expected=$(awk -v name="$_name" '$2 == name || $2 == "*"name { print $1 }' "$_sums_file" | head -1)
    if [ -z "$_expected" ]; then
        error "no SHA256SUMS entry for '$_name' in release $_ver. Set KIMBERLITE_SKIP_CHECKSUM=1 to bypass (not recommended)."
    fi

    _actual=$(sha256_of "$_path") || error "no SHA-256 tool found (need sha256sum, shasum, or openssl). Set KIMBERLITE_SKIP_CHECKSUM=1 to bypass (not recommended)."

    if [ "$_expected" != "$_actual" ]; then
        error "SHA-256 mismatch for $_name: expected $_expected, got $_actual"
    fi

    success "Checksum verified"
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
    info "Platform: $os/$arch"
    info "Fetching latest release version..."
    version=$(resolve_version)
    artifact_name="kimberlite-${os}-${arch}"
    download_url="https://github.com/$REPO/releases/download/$version/${artifact_name}.zip"

    info "Version:  $version"

    # Need unzip
    need_cmd "unzip"

    # Create temp directory
    tmp_dir=$(mktemp -d)
    trap 'rm -rf "$tmp_dir"' EXIT

    # Download
    info "Downloading $artifact_name..."
    download "$download_url" "$tmp_dir/$artifact_name.zip"

    # Verify SHA-256 checksum against the release's SHA256SUMS manifest
    verify_checksum "$version" "$tmp_dir/$artifact_name.zip" "$artifact_name.zip"

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

    # Create `kmb` alias symlink (shorter command name)
    if ln -sf "$install_dir/kimberlite" "$install_dir/kmb" 2>/dev/null; then
        info "Installed alias: kmb -> kimberlite"
    else
        # ln can fail on filesystems without symlink support; try cp as a fallback.
        if cp "$binary" "$install_dir/kmb" 2>/dev/null; then
            info "Installed alias: kmb (copy; symlinks unavailable)"
        else
            info "Skipped kmb alias (install dir not writable for symlinks/copy)"
        fi
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

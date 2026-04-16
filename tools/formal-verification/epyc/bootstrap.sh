#!/usr/bin/env bash
# Bootstrap the EPYC Hetzner host for Kimberlite formal-verification
# campaigns. Idempotent: safe to re-run.
#
# Provisions:
#   - System packages (Java 17, Docker, build tools, graphviz)
#   - Rust toolchain (stable + nightly with miri)
#   - Kani model checker
#   - TLA+ tools (tla2tools.jar v1.8.0, SHA-256 pinned)
#   - Alloy (org.alloytools.alloy.dist.jar v6.2.0, SHA-256 pinned)
#   - TLAPS and Ivy Docker images (built from tools/formal-verification/docker/)
#   - Coq 8.18 Docker image (pulled from coqorg/coq:8.18)
#   - Directory layout under /opt/kimberlite-fv/
#
# Pair this with the `fv-epyc-setup` justfile recipe that invokes it over SSH.

set -euo pipefail

# -----------------------------------------------------------------------------
# Configuration (kept in sync with justfile FV_EPYC_PATH and CI workflow hashes)
# -----------------------------------------------------------------------------
FV_ROOT="${FV_ROOT:-/opt/kimberlite-fv}"
REPO_ROOT="${FV_REPO_ROOT:-${FV_ROOT}/repo}"

TLA_VERSION="1.8.0"
TLA_SHA256="4c1d62e0f67c1d89f833619d7edad9d161e74a54b153f4f81dcef6043ea0d618"

ALLOY_VERSION="6.2.0"
ALLOY_SHA256="6b8c1cb5bc93bedfc7c61435c4e1ab6e688a242dc702a394628d9a9801edb78d"

COQ_IMAGE="coqorg/coq:8.18"

# -----------------------------------------------------------------------------
# Helpers
# -----------------------------------------------------------------------------
log() { printf '\033[1;34m[fv-bootstrap]\033[0m %s\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }

require_root() {
    if [[ $EUID -ne 0 ]]; then
        echo "This script must run as root (needed for apt install, docker setup)." >&2
        exit 1
    fi
}

# -----------------------------------------------------------------------------
# Directory layout
# -----------------------------------------------------------------------------
setup_layout() {
    log "Creating directory layout under ${FV_ROOT}"
    mkdir -p "${FV_ROOT}"/{repo,results,artifacts,tla,alloy}
}

# -----------------------------------------------------------------------------
# System packages
# -----------------------------------------------------------------------------
install_system_packages() {
    log "Installing apt packages (Java, Docker, graphviz, build tools)"
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    apt-get install -y --no-install-recommends \
        ca-certificates curl git build-essential pkg-config \
        openjdk-17-jre-headless \
        docker.io \
        graphviz libgraphviz-dev \
        python3 python3-pip \
        wget xz-utils

    systemctl enable --now docker >/dev/null 2>&1 || true
}

# -----------------------------------------------------------------------------
# Rust toolchain
# -----------------------------------------------------------------------------
install_rust() {
    if ! have rustup; then
        log "Installing rustup + stable toolchain"
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
            | sh -s -- -y --default-toolchain stable --profile minimal
    fi
    # shellcheck disable=SC1091
    source "${HOME}/.cargo/env"

    log "Ensuring stable + nightly toolchains"
    rustup toolchain install stable --profile minimal --component rustfmt clippy
    rustup toolchain install nightly --profile minimal --component miri rust-src
}

# -----------------------------------------------------------------------------
# Kani model checker
# -----------------------------------------------------------------------------
install_kani() {
    # shellcheck disable=SC1091
    source "${HOME}/.cargo/env"
    if ! have cargo-kani; then
        log "Installing Kani model checker"
        cargo install --locked kani-verifier
    fi
    log "Running 'cargo kani setup' (idempotent)"
    cargo kani setup
}

# -----------------------------------------------------------------------------
# TLA+ and Alloy jars (pinned, SHA-256 verified)
# -----------------------------------------------------------------------------
install_tla_alloy() {
    local tla_jar="${FV_ROOT}/tla/tla2tools.jar"
    local alloy_jar="${FV_ROOT}/alloy/alloy-${ALLOY_VERSION}.jar"

    if [[ ! -f "${tla_jar}" ]] || ! echo "${TLA_SHA256}  ${tla_jar}" | sha256sum --check --status; then
        log "Downloading TLA+ tools v${TLA_VERSION}"
        wget -q "https://github.com/tlaplus/tlaplus/releases/download/v${TLA_VERSION}/tla2tools.jar" -O "${tla_jar}"
        echo "${TLA_SHA256}  ${tla_jar}" | sha256sum --check
    else
        log "TLA+ tools jar already present (SHA-256 verified)"
    fi

    if [[ ! -f "${alloy_jar}" ]] || ! echo "${ALLOY_SHA256}  ${alloy_jar}" | sha256sum --check --status; then
        log "Downloading Alloy v${ALLOY_VERSION}"
        wget -q "https://github.com/AlloyTools/org.alloytools.alloy/releases/download/v${ALLOY_VERSION}/org.alloytools.alloy.dist.jar" -O "${alloy_jar}"
        echo "${ALLOY_SHA256}  ${alloy_jar}" | sha256sum --check
    else
        log "Alloy jar already present (SHA-256 verified)"
    fi
}

# -----------------------------------------------------------------------------
# Docker images: TLAPS, Ivy, Coq
# -----------------------------------------------------------------------------
build_docker_images() {
    if [[ ! -d "${REPO_ROOT}" ]]; then
        log "Repo not yet rsync'd to ${REPO_ROOT}; skipping docker image build. Run `just fv-epyc-deploy` first."
        return
    fi

    if ! docker image inspect kimberlite-tlaps:latest >/dev/null 2>&1; then
        log "Building TLAPS Docker image"
        docker build -t kimberlite-tlaps:latest \
            "${REPO_ROOT}/tools/formal-verification/docker/tlaps/"
    fi

    if ! docker image inspect kimberlite-ivy:latest >/dev/null 2>&1; then
        log "Building Ivy Docker image"
        docker build -t kimberlite-ivy:latest \
            "${REPO_ROOT}/tools/formal-verification/docker/ivy/"
    fi

    if ! docker image inspect "${COQ_IMAGE}" >/dev/null 2>&1; then
        log "Pulling Coq image ${COQ_IMAGE}"
        docker pull "${COQ_IMAGE}"
    fi
}

# -----------------------------------------------------------------------------
# Main
# -----------------------------------------------------------------------------
main() {
    require_root
    setup_layout
    install_system_packages
    install_rust
    install_kani
    install_tla_alloy
    build_docker_images
    log "bootstrap complete — formal-verification host ready"
    log "Directory: ${FV_ROOT}"
    log "TLA jar:   ${FV_ROOT}/tla/tla2tools.jar"
    log "Alloy jar: ${FV_ROOT}/alloy/alloy-${ALLOY_VERSION}.jar"
}

main "$@"

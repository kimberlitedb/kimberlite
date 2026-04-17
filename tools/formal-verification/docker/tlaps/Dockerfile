# TLAPS (TLA+ Proof System) Docker Image
#
# Uses the upstream pre-built tlapm binary instead of building from source.
# Building from source (`dune build` of tlapm HEAD) has been fragile across
# OCaml versions, opam package states, and Isabelle JDK packaging. The
# pre-built tarball ships a working tlapm + bundled Isabelle + SMT backends
# with no build step required.
#
# Upstream: https://github.com/tlaplus/tlapm/releases
# Last verified: 2026-04-17 (tlapm 1.6.0-pre)

# Ubuntu 24.04 ships GLIBC 2.39; the upstream tlapm 1.6.0-pre binary is
# linked against GLIBC 2.38+ and will not run on 22.04.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

# Runtime dependencies: Java for Isabelle, zenon/ls4/z3 interpreters, and
# base shell utilities. `default-jre-headless` is sufficient — we don't need
# the JDK because the tarball ships its own jvm.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl wget \
        libgmp10 zlib1g \
        default-jre-headless \
        python3 \
    && rm -rf /var/lib/apt/lists/*

# Pin to an explicit release and SHA-256 to keep the image reproducible.
# Update both TLAPM_VERSION and TLAPM_SHA256 when upgrading.
ARG TLAPM_VERSION=1.6.0-pre
ARG TLAPM_SHA256

# Download + extract the pre-built tarball.
RUN set -eux; \
    cd /opt; \
    wget -q "https://github.com/tlaplus/tlapm/releases/download/${TLAPM_VERSION}/tlapm-${TLAPM_VERSION}-x86_64-linux-gnu.tar.gz"; \
    if [ -n "${TLAPM_SHA256:-}" ]; then \
        echo "${TLAPM_SHA256}  tlapm-${TLAPM_VERSION}-x86_64-linux-gnu.tar.gz" | sha256sum --check; \
    fi; \
    tar xzf "tlapm-${TLAPM_VERSION}-x86_64-linux-gnu.tar.gz"; \
    rm "tlapm-${TLAPM_VERSION}-x86_64-linux-gnu.tar.gz"; \
    ln -s "/opt/tlapm-${TLAPM_VERSION}" /opt/tlapm

ENV PATH="/opt/tlapm/bin:${PATH}"

# Sanity: tlapm --version should print the pinned version.
RUN tlapm --version

WORKDIR /workspace

ENTRYPOINT ["tlapm"]

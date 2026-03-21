# TLAPS (TLA+ Proof System) Docker Image
#
# STATUS: Non-functional on ARM (aarch64) — upstream Isabelle JDK issue.
# TLAPS's dune build unconditionally downloads Isabelle2025, which bundles a
# JDK that lacks ARM Linux support (jdk-21.0.6/arm64-linux/bin/java missing).
#
# This image builds successfully on x86_64 hosts/CI runners.
# On ARM (Graviton, Apple Silicon), TLAPS verification is skipped.
#
# Upstream tracking: https://github.com/tlaplus/tlapm
# Last verified: 2026-03-21

FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive

# Install system dependencies for OCaml / opam / TLAPS
RUN apt-get update && apt-get install -y \
    curl wget git ca-certificates \
    build-essential gcc g++ make \
    opam \
    libgmp-dev \
    zlib1g-dev \
    m4 \
    patch \
    rsync \
    unzip \
    default-jdk \
    && rm -rf /var/lib/apt/lists/*

# Initialize opam with OCaml 4.14.2 (required by tlapm)
RUN opam init --disable-sandboxing --bare -y && \
    opam switch create 4.14.2 --packages ocaml-base-compiler.4.14.2 && \
    eval $(opam env) && \
    opam update

# Build TLAPS from source.
RUN eval $(opam env) && \
    opam install --yes dune sexplib && \
    git clone --depth 1 https://github.com/tlaplus/tlapm.git /opt/tlapm && \
    cd /opt/tlapm && \
    eval $(opam env) && \
    opam install --yes --deps-only . && \
    dune build && \
    dune install

# Add opam bin directory to PATH for all subsequent commands
ENV PATH="/root/.opam/4.14.2/bin:${PATH}"

# Verify installation
RUN tlapm --version

WORKDIR /workspace

ENTRYPOINT ["tlapm"]

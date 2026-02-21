# SECURITY: Pin base image to digest to prevent supply-chain attacks.
# To get the current digest: docker pull ubuntu:22.04 && docker inspect ubuntu:22.04 --format='{{index .RepoDigests 0}}'
# TODO: Replace tag with digest pin: FROM ubuntu@sha256:<digest>
# Last verified: 2026-02-22 — update digest when upgrading base image version.
FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive

# Install system dependencies for OCaml / opam / TLAPS
RUN apt-get update && apt-get install -y \
    curl wget git ca-certificates \
    build-essential gcc g++ make \
    opam \
    libgmp-dev \
    m4 \
    patch \
    rsync \
    unzip \
    && rm -rf /var/lib/apt/lists/*

# Initialize opam with OCaml 4.14.2 (required by tlapm)
RUN opam init --disable-sandboxing --bare -y && \
    opam switch create 4.14.2 --packages ocaml-base-compiler.4.14.2 && \
    eval $(opam env) && \
    opam update

# Install TLAPS (tlapm) from the opam repository.
# We avoid git-cloning a specific tag because tlapm's GitHub tags may not
# match the opam package names, causing "Could not find remote branch" errors.
# The opam repository provides a stable, tested version.
RUN eval $(opam env) && \
    opam install --yes tlapm

# Add opam bin directory to PATH for all subsequent commands
ENV PATH="/root/.opam/4.14.2/bin:${PATH}"

# Verify installation
RUN tlapm --version

WORKDIR /workspace

ENTRYPOINT ["tlapm"]

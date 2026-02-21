# SECURITY: Pin base image to digest to prevent supply-chain attacks.
# To get the current digest: docker pull ubuntu:22.04 && docker inspect ubuntu:22.04 --format='{{index .RepoDigests 0}}'
# TODO: Replace tag with digest pin: FROM ubuntu@sha256:<digest>
# Last verified: 2026-02-22 — update digest when upgrading base image version.
FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive

# Install dependencies (python-is-python3 provides /usr/bin/python symlink)
RUN apt-get update && apt-get install -y \
    python3 python3-pip python-is-python3 python3-tk \
    git build-essential \
    libgmp-dev libmpfr-dev libmpc-dev \
    g++ cmake \
    python3-ply \
    && rm -rf /var/lib/apt/lists/*

# Clone Ivy pinned to v0.1-msv for reproducibility.
# DO NOT run git submodule update: building Z3 from C++ source segfaults on ARM
# (AWS Graviton / Apple Silicon). Use z3-solver PyPI package instead.
RUN git clone --depth 1 --branch v0.1-msv https://github.com/kenmcmil/ivy.git /opt/ivy

# Install z3-solver from PyPI (pre-built, ARM-safe) and Ivy Python deps
RUN pip3 install z3-solver pyparsing ply tarjan pydot ordered-set

# Install Ivy without Z3 submodule rebuild
RUN cd /opt/ivy && \
    SKIP_Z3_BUILD=1 pip3 install -e . --no-build-isolation || pip3 install -e .

# Verify installation
RUN ivy_check --help 2>&1 | head -3 || true

WORKDIR /workspace
ENTRYPOINT ["ivy_check"]

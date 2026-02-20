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

# Initialize opam with OCaml 4.14.2 (required by tlapm 1.5.0)
RUN opam init --disable-sandboxing --bare -y && \
    opam switch create 4.14.2 --packages ocaml-base-compiler.4.14.2 && \
    eval $(opam env) && \
    opam update

# Install TLAPS (tlapm) pinned to v1.5.0 for reproducibility
# Source: https://github.com/tlaplus/tlapm/releases/tag/v1.5.0
RUN eval $(opam env) && \
    opam pin add --yes tlapm https://github.com/tlaplus/tlapm.git#v1.5.0 && \
    opam install --yes tlapm

# Add opam bin directory to PATH for all subsequent commands
ENV PATH="/root/.opam/4.14.2/bin:${PATH}"

# Verify installation
RUN tlapm --version

WORKDIR /workspace

ENTRYPOINT ["tlapm"]

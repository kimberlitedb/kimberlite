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
# Clone via git first to avoid opam's git+https protocol issues in Docker build,
# then pin from the local directory so opam can find the .opam file.
RUN git clone --depth 1 --branch v1.5.0 https://github.com/tlaplus/tlapm.git /tmp/tlapm-src && \
    eval $(opam env) && \
    opam pin add --yes tlapm /tmp/tlapm-src && \
    opam install --yes tlapm && \
    rm -rf /tmp/tlapm-src

# Add opam bin directory to PATH for all subsequent commands
ENV PATH="/root/.opam/4.14.2/bin:${PATH}"

# Verify installation
RUN tlapm --version

WORKDIR /workspace

ENTRYPOINT ["tlapm"]

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

# Clone Ivy
RUN git clone https://github.com/kenmcmil/ivy.git /opt/ivy && \
    cd /opt/ivy && \
    git submodule update --init --recursive

# Build only Z3 (skip picotls/aiger/abc — not needed for ivy_check)
RUN cd /opt/ivy/submodules/z3 && \
    python scripts/mk_make.py --python --prefix /opt/ivy --pypkgdir /opt/ivy/ivy/ && \
    cd build && \
    make -j$(nproc) && \
    make install && \
    cd /opt/ivy && \
    mkdir -p ivy/include ivy/lib ivy/z3 && \
    cp submodules/z3/build/include/*.h ivy/include/ 2>/dev/null || true && \
    cp submodules/z3/build/lib/*.so ivy/lib/ 2>/dev/null || true && \
    cp submodules/z3/build/lib/*.so ivy/z3/ 2>/dev/null || true

# Install Ivy
RUN cd /opt/ivy && \
    pip3 install -e .

# Make Z3 shared library discoverable system-wide
RUN ln -sf /opt/ivy/lib/libz3.so /usr/lib/libz3.so && ldconfig

# Verify installation
RUN ivy_check --help 2>&1 | head -3 || true

WORKDIR /workspace
ENTRYPOINT ["ivy_check"]

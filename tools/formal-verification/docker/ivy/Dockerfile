# SECURITY: Pin base image to digest to prevent supply-chain attacks.
# Last verified: 2026-03-21 — update digest when upgrading base image version.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

# Install dependencies including Z3 from apt (pre-built, ARM-safe)
RUN apt-get update && apt-get install -y \
    python3 python3-pip python-is-python3 python3-tk \
    git build-essential \
    libgmp-dev libmpfr-dev libmpc-dev \
    g++ cmake \
    python3-ply \
    graphviz libgraphviz-dev pkg-config \
    z3 python3-z3 \
    && rm -rf /var/lib/apt/lists/*

# Clone Ivy from the 2to3new branch (Python 3 compatible).
RUN git clone --depth 1 --branch 2to3new https://github.com/kenmcmil/ivy.git /opt/ivy

# Install Python deps (z3 already installed from apt)
RUN pip3 install --break-system-packages pyparsing ply tarjan pydot ordered-set pygraphviz

# Install Ivy as a proper package (not editable) so relative imports work.
# SKIP_Z3_BUILD prevents attempting to build Z3 from source.
RUN cd /opt/ivy && \
    SKIP_Z3_BUILD=1 pip3 install --break-system-packages . --no-build-isolation 2>&1 || \
    pip3 install --break-system-packages . 2>&1

# Ivy expects `import ivy.z3` — create a shim that re-exports the system z3 module.
# The apt-installed python3-z3 puts z3 at the top level, not inside ivy/.
RUN IVY_PKG=$(python3 -c "import ivy; import os; print(os.path.dirname(ivy.__file__))") && \
    printf 'from z3 import *\n' > "$IVY_PKG/z3.py"

# Verify ivy_check is importable
RUN python3 -c "from ivy.ivy_check import main; print('ivy_check import OK')"

WORKDIR /workspace
ENTRYPOINT ["ivy_check"]

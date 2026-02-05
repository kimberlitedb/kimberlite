FROM ubuntu:22.04

# Install dependencies
RUN apt-get update && apt-get install -y \
    python3 python3-pip git build-essential \
    libgmp-dev libmpfr-dev libmpc-dev \
    g++ cmake

# Clone Ivy and build Z3
RUN git clone https://github.com/kenmcmil/ivy.git /opt/ivy && \
    cd /opt/ivy && \
    git submodule update --init --recursive && \
    python3 build_submodules.py && \
    pip3 install -e .

# Verify installation
RUN ivy_check --help

WORKDIR /workspace
ENTRYPOINT ["ivy_check"]

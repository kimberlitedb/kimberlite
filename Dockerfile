# Kimberlite Database Server
# Multi-stage build for minimal production image
#
# Build:  docker build -t kimberlite .
# Run:    docker run -p 5432:5432 -v kimberlite-data:/data kimberlite

# --- Builder stage ---
# Digest pinned to rust:1.88-slim linux/amd64 (AUDIT-2026-03 5.4 — supply-chain hardening)
FROM rust:1.88-slim@sha256:a6cab604fa016ac022e78c24038497eb7617ab59150ca4c3dd2ede0fbd514d4b AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependencies by copying manifests first
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/

# Build only the CLI binary in release mode
RUN cargo build --release --profile release-official -p kimberlite-cli \
    && strip /build/target/release-official/kimberlite

# --- Runtime stage ---
# Digest pinned to debian:bookworm-slim linux/amd64 (AUDIT-2026-03 5.4 — supply-chain hardening)
FROM debian:bookworm-slim@sha256:6458e6ce2b6448e31bfdced4be7d8aa88d389e6694ab09f5a718a694abe147f4

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r kimberlite && useradd -r -g kimberlite -d /data -s /sbin/nologin kimberlite

COPY --from=builder /build/target/release-official/kimberlite /usr/local/bin/kimberlite
RUN ln -s /usr/local/bin/kimberlite /usr/local/bin/kmb

# Create data directory
RUN mkdir -p /data && chown kimberlite:kimberlite /data
VOLUME /data

# Kimberlite server port
EXPOSE 5432

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD kimberlite info --server 127.0.0.1:5432 --tenant 0 || exit 1

USER kimberlite
WORKDIR /data

ENTRYPOINT ["kimberlite"]
CMD ["start", "/data"]

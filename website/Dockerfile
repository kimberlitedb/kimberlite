# syntax=docker/dockerfile:1

# KimberliteDB Website Container
# Multi-stage build for minimal image size

# Stage 1: Build the Rust binary
# Digest pinned to rust:1.88-bookworm linux/amd64 (AUDIT-2026-03 5.4 — supply-chain hardening)
FROM --platform=linux/amd64 rust:1.88-bookworm@sha256:4727898c104ecd2e22d780925832502faee9fe4e70581b8572af081370b315a0 AS builder

# Build version for cache busting (git commit hash passed at build time)
ARG BUILD_VERSION=unknown

WORKDIR /app

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

# Copy assets needed at build time (CSS minification + Askama templates)
COPY public ./public
COPY templates ./templates

# Build the release binary with cache-busting version
ENV BUILD_VERSION=${BUILD_VERSION}
RUN cargo build --release -p kmb-site

# Stage 2: Runtime image
# Digest pinned to debian:bookworm-slim linux/amd64 (AUDIT-2026-03 5.4 — supply-chain hardening)
FROM --platform=linux/amd64 debian:bookworm-slim@sha256:6458e6ce2b6448e31bfdced4be7d8aa88d389e6694ab09f5a718a694abe147f4

# Install CA certificates for HTTPS requests (if needed in future)
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/kmb-site ./

# Copy static assets, templates, and docs
COPY public ./public
COPY content ./content
COPY templates ./templates
# Docs are copied from repo root before build: cp -r ../docs ./docs
COPY docs ./docs

# Environment configuration
ENV HOST=0.0.0.0
ENV PORT=3000
ENV RUST_LOG=info
ENV DOCS_PATH=/app/docs

# Expose the port
EXPOSE 3000

# Run the server
CMD ["./kmb-site"]

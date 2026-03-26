# ── Stage 1: Build ────────────────────────────────────────────────────────────
# Uses the official Rust image which includes cargo, rustup, and all build deps.
FROM rust:1.83 AS builder

# Install cargo-leptos and the WASM target.
RUN rustup target add wasm32-unknown-unknown && \
    cargo install cargo-leptos --locked

WORKDIR /app

# Copy manifests first for better layer caching.
COPY Cargo.toml Cargo.lock ./
COPY app/Cargo.toml app/

# Pre-fetch dependencies (cache layer).
RUN mkdir -p app/src && \
    echo 'fn main(){}' > app/src/main.rs && \
    echo '' > app/src/lib.rs && \
    cargo fetch

# Copy the full source tree.
COPY app/src app/src

# Build the release binary and WASM assets.
RUN cargo leptos build --release 2>&1

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
# Minimal Debian image — only the compiled binary and static assets.
FROM debian:bookworm-slim AS runtime

# Install CA certificates for outbound TLS (metadata fetcher).
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Create a non-root user for the application process.
RUN useradd -ms /bin/bash procastimarks
USER procastimarks

WORKDIR /app

# Copy the server binary.
COPY --from=builder /app/target/release/procastimarks /app/procastimarks

# Copy the compiled WASM bundle and static assets.
COPY --from=builder /app/target/site /app/site

# The SQLite database is stored on a named volume mounted at /data.
VOLUME ["/data"]

EXPOSE 3000

ENV DATABASE_URL=/data/bookmarks.db
ENV BIND_ADDRESS=0.0.0.0:3000
ENV LEPTOS_SITE_ROOT=/app/site

CMD ["/app/procastimarks"]

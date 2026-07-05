# syntax=docker/dockerfile:1

# -----------------------------
# Stage 1: Build
# -----------------------------
FROM rust:1.90-slim AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && \
    apt-get upgrade -y && \
    apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
        ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Copy manifests first (better Docker layer caching)
COPY Cargo.toml Cargo.lock ./

# Create a dummy source so dependencies can be cached
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release || true

# Remove dummy source
RUN rm -rf src

# Copy the rest of the project
COPY . .

# Build release binary
RUN cargo build --release

# -----------------------------
# Stage 2: Runtime
# -----------------------------
FROM debian:13.5-slim AS runtime

WORKDIR /app

# Update packages and install only what's required
RUN apt-get update && \
    apt-get upgrade -y && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        libssl3 && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd -r appuser && \
    useradd -r -g appuser appuser

# Copy binary
COPY --from=builder /app/target/release/rust-video-sdk ./rust-video-sdk

# Ensure binary is executable
RUN chmod +x ./rust-video-sdk

# Use non-root user
USER appuser

# Expose Axum port
EXPOSE 3000

# Optional healthcheck
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD ["./rust-video-sdk", "--help"]

# Start application
CMD ["./rust-video-sdk"]
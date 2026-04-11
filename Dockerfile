# --- Stage 1: Build ---
FROM rust:1.88-slim AS builder

WORKDIR /app

# Install build dependencies (needed for crates like sqlx and openssl)
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy your source code
COPY . .

# Build for release
RUN cargo build --release

# --- Stage 2: Run ---
FROM debian:bookworm-slim AS runtime

WORKDIR /app

# Install runtime dependencies (OpenSSL is usually required by sqlx/rustls)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy only the binary from the builder stage
# Replace 'rust-video-sdk' with the exact name in your Cargo.toml [package]
COPY --from=builder /app/target/release/rust-video-sdk /app/rust-video-sdk

# Expose the port your Axum server is listening on
EXPOSE 3000

# Run the binary
CMD ["./rust-video-sdk"]
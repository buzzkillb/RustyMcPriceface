# Build stage - compile Rust binaries for musl
FROM rust:1.82-alpine AS builder

# Install build dependencies
RUN apk add --no-cache \
    musl-dev \
    pkgconfig \
    openssl-dev \
    openssl-libs-static

WORKDIR /app

# Copy source code
COPY Cargo.toml ./
COPY Cargo.lock* ./
COPY src/ ./src/

# Build for musl target (static linking)
ENV RUSTFLAGS="-C target-feature=+crt-static"
RUN cargo build --release --target x86_64-unknown-linux-musl

# Runtime stage - minimal Alpine image
FROM alpine:3.19

# Install only runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    curl \
    jq \
    && addgroup -g 1000 appuser \
    && adduser -D -s /bin/sh -u 1000 -G appuser appuser

WORKDIR /app

# Copy binaries from builder stage
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/price-service ./price-service
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/discord-bot ./discord-bot
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/db-query ./db-query
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/db-cleanup ./db-cleanup

# Create shared directory and set permissions
RUN mkdir -p /app/shared \
    && chown -R appuser:appuser /app

# Switch to non-root user for security
USER appuser

# Default command
CMD ["./price-service"] 

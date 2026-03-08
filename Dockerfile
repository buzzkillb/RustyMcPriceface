# Build stage - compile Rust binaries
FROM rust:slim-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libfreetype6-dev \
    libfontconfig1-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy manifests first
COPY Cargo.toml Cargo.lock ./

# Create dummy src/main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN echo "fn main() {}" > src/db_query.rs
RUN echo "fn main() {}" > src/db_cleanup.rs
RUN echo "fn main() {}" > src/price_service.rs
RUN echo "fn main() {}" > src/shanghai_price_service.rs

# Build dependencies
RUN cargo build --release

# Now remove dummy source
RUN rm -rf src

# Copy actual source code
COPY src/ ./src/

# Build the actual application
# We touch the main files to ensure cargo rebuilds them
RUN touch src/main.rs src/price_service.rs src/shanghai_price_service.rs
# Build only the main discord-bot binary
RUN cargo build --release --bin discord-bot

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    jq \
    libfreetype6 \
    libfontconfig1 \
    fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r appuser && useradd -r -g appuser appuser

WORKDIR /app

# Copy the single monolithic binary
COPY --from=builder /app/target/release/discord-bot ./rustymcpriceface

# Create shared directory and set permissions
RUN mkdir -p /app/shared \
    && chown -R appuser:appuser /app

# Switch to non-root user for security
USER appuser

# Expose port for health check
EXPOSE 8080

# Default command
CMD ["./rustymcpriceface"]

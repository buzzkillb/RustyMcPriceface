FROM debian:bookworm-slim

# Install runtime dependencies including OpenSSL and curl for health checks
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the built binaries from local build
COPY target/release/price-service /app/price-service
COPY target/release/discord-bot /app/discord-bot
COPY target/release/db-query /app/db-query

# Make binaries executable
RUN chmod +x /app/price-service /app/discord-bot /app/db-query

# Create shared directory for price data
RUN mkdir -p /app/shared

# Default command (can be overridden)
CMD ["/app/price-service"] 

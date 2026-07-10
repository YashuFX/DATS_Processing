# ── Stage 1: Build ────────────────────────────────────────────────────────────
FROM rust:1.78-slim-bookworm AS builder

# Install system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the entire workspace context
COPY . .

WORKDIR /app/engineering-conversion-service

# Build the release binary
RUN cargo build --release

# ── Stage 2: Run ─────────────────────────────────────────────────────────────
FROM gcr.io/distroless/cc-debian12 AS runner

WORKDIR /app

# Copy the compiled binary
COPY --from=builder /app/engineering-conversion-service/target/release/engineering-conversion-service /app/engineering-conversion-service

# Expose metrics port
EXPOSE 8085

# Define default environment variables
ENV AMQP_URL="amqp://guest:guest@localhost:5672/%2f" \
    SOURCE_EXCHANGE="telemetry.engineering" \
    SOURCE_QUEUE="engineering.convert" \
    SOURCE_ROUTING_KEY="#.decommutated" \
    DESTINATION_EXCHANGE="telemetry.engineering" \
    DERIVED_DB_DIR="/etc/must/derived" \
    PREFETCH_COUNT="50" \
    PUBLISH_TIMEOUT_MS="5000" \
    RETRY_MAX_ATTEMPTS="5" \
    METRICS_PORT="8085" \
    CONSUMER_TAG="ecs-converter-1" \
    RUST_LOG="info"

ENTRYPOINT ["/app/engineering-conversion-service"]

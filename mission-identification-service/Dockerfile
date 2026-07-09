# ── Stage 1: Build ────────────────────────────────────────────────────────────
FROM rust:1.78-slim-bookworm AS builder

# Install system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the entire workspace context (since we have a relative build.rs referencing shared proto)
COPY . .

WORKDIR /app/mission-identification-service

# Build the release binary
RUN cargo build --release

# ── Stage 2: Run ─────────────────────────────────────────────────────────────
FROM gcr.io/distroless/cc-debian12 AS runner

WORKDIR /app

# Copy the compiled binary
COPY --from=builder /app/mission-identification-service/target/release/mission-identification-service /app/mission-identification-service
# Copy the default rules configuration registry
COPY --from=builder /app/mission-identification-service/configs/registry.yaml /app/configs/registry.yaml

# Expose metrics port
EXPOSE 8083

# Define default environment variables
ENV AMQP_URL="amqp://guest:guest@localhost:5672/%2f" \
    REGISTRY_FILE_PATH="/app/configs/registry.yaml" \
    SOURCE_EXCHANGE="telemetry.decoded" \
    SOURCE_QUEUE="mission.identify" \
    SOURCE_ROUTING_KEY="#.decoded" \
    DESTINATION_EXCHANGE="telemetry.identified" \
    PREFETCH_COUNT="50" \
    PUBLISH_TIMEOUT_MS="5000" \
    RETRY_MAX_ATTEMPTS="5" \
    METRICS_PORT="8083" \
    CONSUMER_TAG="mission-identification-service-1" \
    RUST_LOG="info"

ENTRYPOINT ["/app/mission-identification-service"]

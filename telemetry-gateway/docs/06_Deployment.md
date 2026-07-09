# MuST Telemetry Gateway — Deployment Document

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-GW-DEP-006                          |
| **Version**        | 1.0.0-DRAFT                             |
| **Date**           | 2026-07-09                               |
| **Status**         | DRAFT — PENDING REVIEW                   |

---

## 1. Multi-Stage Dockerfile Reference

```dockerfile
# =========================================================
# Stage 1: Build Environment
# =========================================================
FROM rust:1.75-slim AS builder

WORKDIR /usr/src/app

# Install system dependencies (e.g. protobuf compiler for tonic-build)
RUN apt-get update && apt-get install -y protobuf-compiler && rm -rf /var/lib/apt/lists/*

# Copy workspace and manifests
COPY Cargo.toml Cargo.lock ./

# Create dummy src file to cache dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release

# Copy actual source files and build
COPY . .
RUN touch src/main.rs && cargo build --release

# =========================================================
# Stage 2: Distroless Runtime Environment
# =========================================================
FROM gcr.io/distroless/cc-debian12:latest

# Expose gRPC Ingress port
EXPOSE 50052

# Copy compiled binary
COPY --from=builder /usr/src/app/target/release/telemetry-gateway /telemetry-gateway

# Set execution context
USER 65534:65534
ENTRYPOINT ["/telemetry-gateway"]
```

---

## 2. Environment Configuration

The Telemetry Gateway is configured primarily through environment variables:

| Environment Variable | Default Value | Description |
|----------------------|---------------|-------------|
| `AMQP_URL` | `amqp://guest:guest@127.0.0.1:5672/%2f` | RabbitMQ connection URL |
| `RUST_LOG` | `info` | Logging verbosity filter (`debug`, `info`, `warn`, `error`) |

---

## 3. Observability Specs

### 3.1 Prometheus Metrics

| Metric Name | Type | Labels | Description |
|-------------|------|--------|-------------|
| `gateway_packets_received_total` | Counter | `source_id` | Total telemetry packets received. |
| `gateway_packets_published_total` | Counter | `source_id` | Total packets pushed to RabbitMQ. |
| `gateway_packets_dropped_total` | Counter | `reason` | Count of dropped packets (e.g., validation failures). |
| `gateway_processing_latency_seconds` | Histogram | None | Processing latency from source generation to Gateway publish. |

### 3.2 Log Fields (Structured JSON via `tracing-subscriber`)

All logs outputted by the gateway contain these structured metadata fields:
- `timestamp`: Log event execution time
- `level`: Log level (INFO, WARN, ERROR)
- `message`: Log text description
- `target`: Rust module path
- `source_id`: Telemetry source ID (if applicable)
- `key`: RabbitMQ routing key (on successful publication)
- `envelope_id`: Unique UUID of the telemetry envelope

---

## 4. Kubernetes Probes

For a gRPC-only service, liveness and readiness are validated via the standard gRPC Health Checking Protocol using the `grpc-health-probe` tool:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: must-telemetry-gateway
  namespace: must
spec:
  replicas: 3
  template:
    spec:
      containers:
      - name: telemetry-gateway
        image: must/telemetry-gateway:1.0.0
        ports:
        - containerPort: 50052
          name: grpc
        livenessProbe:
          exec:
            command: ["/bin/grpc-health-probe", "-addr=:50052"]
          initialDelaySeconds: 10
          periodSeconds: 15
        readinessProbe:
          exec:
            command: ["/bin/grpc-health-probe", "-addr=:50052"]
          initialDelaySeconds: 5
          periodSeconds: 10
```

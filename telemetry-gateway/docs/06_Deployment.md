# MuST Telemetry Gateway — Deployment Document

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-GW-DEP-006                          |
| **Version**        | 1.0.0-DRAFT                             |
| **Date**           | 2026-07-03                               |
| **Status**         | DRAFT — PENDING REVIEW                   |

---

## 1. Multi-Stage Dockerfile

```dockerfile
# =========================================================
# Stage 1: Build Environment
# =========================================================
FROM golang:1.22-alpine AS builder

# Install system dependencies needed for compiling
RUN apk add --no-cache git make build-base

WORKDIR /app

# Copy dependency manifests first to leverage Docker layer caching
COPY go.mod go.sum ./
RUN go mod download

# Copy source tree
COPY . .

# Compile optimized static binary
RUN CGO_ENABLED=0 GOOS=linux go build \
    -ldflags="-w -s -X main.version=1.0.0" \
    -o /bin/telemetry-gateway \
    cmd/gateway/main.go

# =========================================================
# Stage 2: Distroless Minimal Runtime Environment
# =========================================================
FROM gcr.io/distroless/static-debian12:latest

# Expose HTTP, gRPC, and WebSocket ports
EXPOSE 8080 9090

# Copy compiled binary and default configuration
COPY --from=builder /bin/telemetry-gateway /telemetry-gateway
COPY --from=builder /app/configs/production.yaml /configs/production.yaml

# Set container execution context
USER 65534:65534
ENTRYPOINT ["/telemetry-gateway"]
CMD ["--config=/configs/production.yaml"]
```

---

## 2. YAML Configuration

```yaml
service:
  name: "must-telemetry-gateway"
  environment: "production"
  version: "1.0.0"
  log_level: "info" # debug, info, warn, error

server:
  http:
    port: 8080
    read_timeout_ms: 5000
    write_timeout_ms: 10000
  grpc:
    port: 9090
    max_concurrent_streams: 1000
    keepalive_time_seconds: 30

rabbitmq:
  hosts: ["amqp://guest:guest@rabbitmq:5672/"]
  connection_timeout_ms: 5000
  heartbeat_seconds: 60
  publish_confirm_timeout_ms: 1000
  retry_limit: 3
  backoff_multiplier_ms: 100

buffer:
  max_memory_bytes: 1610612736 # 1.5 GB
  max_packets: 500000
  spill_to_disk: false

metrics:
  enabled: true
  path: "/metrics"
  port: 8080

tracing:
  enabled: true
  endpoint: "otel-collector:4317"
  sample_rate: 0.1
```

---

## 3. Observability Specs

### 3.1 Prometheus Metrics

| Metric Name | Type | Labels | Description |
|-------------|------|--------|-------------|
| `gateway_packets_received_total` | Counter | `source_id`, `mission_code` | Total telemetry packets received. |
| `gateway_packets_published_total` | Counter | `source_id`, `mission_code` | Total packets pushed to RabbitMQ. |
| `gateway_packets_rejected_total` | Counter | `reason` | Count of rejected packets by validation rule. |
| `gateway_processing_latency_seconds` | Histogram | `source_id` | End-to-end processing time in the gateway. |
| `gateway_buffer_utilization_pct` | Gauge | None | Current memory queue saturation percentage. |
| `gateway_active_sessions` | Gauge | None | Number of active ingestion sessions. |
| `gateway_rabbitmq_connected` | Gauge | None | RabbitMQ connection status (1 = ok, 0 = lost). |

### 3.2 Log Fields (Structured JSON via Zap)

All logs outputted by the gateway contain these structured metadata fields:
- `timestamp`: RFC3339Nan
- `level`: Log level
- `caller`: File path and line number
- `message`: Log text
- `service_name`: `must-telemetry-gateway`
- `session_id`: Unique identifier (if inside a telemetry stream context)
- `source_id`: Telemetry source ID (if applicable)
- `error_details`: Exception stack or native system message (if error level)

---

## 4. Kubernetes Probes

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
        - containerPort: 8080
          name: http
        - containerPort: 9090
          name: grpc
        startupProbe:
          httpGet:
            path: /health/startup
            port: 8080
          failureThreshold: 30
          periodSeconds: 10
        livenessProbe:
          httpGet:
            path: /health/live
            port: 8080
          initialDelaySeconds: 15
          periodSeconds: 20
        readinessProbe:
          httpGet:
            path: /health/ready
            port: 8080
          initialDelaySeconds: 10
          periodSeconds: 10
```

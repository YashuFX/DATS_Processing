# CCSDS Decoder Service — Deployment Document

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-DEC-DEP-004                         |
| **Version**        | 1.0.0                                    |
| **Date**           | 2026-07-09                               |
| **Status**         | APPROVED                                 |

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
# Stage 2: Distroless Minimal Runtime Environment
# =========================================================
FROM gcr.io/distroless/cc-debian12:latest

# Copy compiled binary
COPY --from=builder /usr/src/app/target/release/ccsds-decoder /ccsds-decoder

# Set container execution context
USER 65534:65534
ENTRYPOINT ["/ccsds-decoder"]
```

---

## 2. Environment Variables Configuration

The CCSDS Decoder is entirely configured using environment variables:

| Environment Variable | Default Value | Description |
|----------------------|---------------|-------------|
| `AMQP_URL` | *(None; Required)* | Full AMQP connection URI (e.g., `amqp://guest:guest@localhost:5672/%2f`) |
| `SOURCE_EXCHANGE` | `telemetry.raw` | Exchange from which raw envelopes are consumed |
| `SOURCE_QUEUE` | `ccsds-decoder.raw` | Name of the bound queue to consume from |
| `SOURCE_ROUTING_KEY` | `#` | Routing key filter for queue binding (default is all keys) |
| `CONSUMER_TAG` | `ccsds-decoder-1` | Unique AMQP consumer identification |
| `PREFETCH_COUNT` | `10` | QoS prefetch window size (messages pre-delivered before ACK) |
| `CHECK_CRC` | `false` | Enable or disable Space Packet 16-bit CRC validation |
| `DESTINATION_EXCHANGE` | `telemetry.decoded` | Exchange to publish mutated and decoded envelopes |
| `PUBLISH_TIMEOUT_MS` | `5000` | Max milliseconds to wait for publisher confirms |
| `RETRY_MAX_ATTEMPTS` | `5` | Max retries for outbound publish attempts |
| `RUST_LOG` | `info` | Logger verbosity filter (`debug`, `info`, `warn`, `error`) |

---

## 3. Observability Specs

### 3.1 Structured Logs

All logs are written via the `tracing` framework to standard output. Example log variables injected on telemetry processing:
- `envelope_id`: Unique UUID of the envelope
- `sequence_number`: Pipeline-wide tracking count
- `apid`: CCSDS Space Packet Application Process Identifier
- `seq_count`: CCSDS Sequence Counter value
- `is_gap`: Boolean flag indicating if a gap was detected
- `crc_ok`: Boolean flag indicating if the CRC is correct

---

## 4. Kubernetes Probes

Because the CCSDS Decoder is a background consumer (without listening HTTP or gRPC servers), standard health checks are configured using container-level command executor probes or process presence checks:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: must-ccsds-decoder
  namespace: must
spec:
  replicas: 2
  template:
    spec:
      containers:
      - name: ccsds-decoder
        image: must/ccsds-decoder:1.0.0
        env:
        - name: AMQP_URL
          value: "amqp://guest:guest@rabbitmq:5672/%2f"
        livenessProbe:
          exec:
            command:
            - /bin/sh
            - -c
            - "pgrep ccsds-decoder"
          initialDelaySeconds: 5
          periodSeconds: 10
        readinessProbe:
          exec:
            command:
            - /bin/sh
            - -c
            - "pgrep ccsds-decoder"
          initialDelaySeconds: 5
          periodSeconds: 10
```

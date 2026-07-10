# XTCE Decoder Service — Operations, Testing, and Roadmap

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-XTCE-OPS-004                        |
| **Version**        | 1.0.0                                    |
| **Date**           | 2026-07-10                               |
| **Status**         | PROPOSED                                 |

---

## 1. Configuration Specification

All configurations are loaded dynamically from environment variables at startup. If a required variable is missing, the service terminates immediately (fail-fast).

| Environment Variable | Required | Default Value | Description |
|----------------------|----------|---------------|-------------|
| `AMQP_URL`           | **Yes**  | N/A           | AMQP Broker connection URI. |
| `SOURCE_EXCHANGE`    | No       | `telemetry.identified` | Exchange to consume identified packets from. |
| `SOURCE_QUEUE`       | No       | `xtce.process` | Queue name to bind and consume. |
| `SOURCE_ROUTING_KEY` | No       | `#.identified`| Binding key pattern. |
| `CONSUMER_TAG`       | No       | `xtce-decoder-1` | Unique consumer tag sent to RabbitMQ. |
| `PREFETCH_COUNT`     | No       | `50`          | QoS prefetch window. |
| `DESTINATION_EXCHANGE` | No     | `telemetry.engineering` | Downstream exchange for engineering envelopes. |
| `XTCE_DB_DIR`        | No       | `/etc/must/xtce` | Directory path containing XTCE XML files. |
| `PUBLISH_TIMEOUT_MS` | No       | `5000`        | Timeout waiting for publish confirmations. |
| `RETRY_MAX_ATTEMPTS` | No       | `5`           | Maximum retries for publishing before failing. |

---

## 2. Validation Rules

To ensure system integrity, validation rules are executed at two levels:

### 2.1 Schema Validation (Load Time)
When loading an XML database for a mission:
- **XML Well-Formedness**: Must parse successfully using XML reader.
- **Root Element Check**: Root element must be `SpaceSystem`.
- **Containers Layout**: Offset positions and bit sizes must not represent negative lengths.
- **Reference Resolution**: All `parameterRef` elements in containers must correspond to a valid parameter name defined in the `ParameterSet`.

### 2.2 Packet Validation (Runtime)
When processing an incoming packet:
- **Payload Bounds Checking**: The bit-level offsets of parameters defined in the matching `SequenceContainer` must not exceed the actual physical byte length of the received telemetry packet.
- **Mission Field Present**: The envelope must contain a valid mission code.
- **Numeric Limits**: Extracted bits must map to types without overflowing target integer or float storage sizes.

---

## 3. Error Handling Strategy

| Error Scenario | Root Cause | System Response | Recovery Action |
|----------------|------------|-----------------|-----------------|
| **Deserialization Failure** | Corrupt envelope payload from RabbitMQ. | Log error at `ERROR` level; send `NACK` to RabbitMQ with `requeue=false`. | Packet is automatically routed to Dead Letter Queue (`must.dlx`). |
| **Missing XTCE File** | Mission code has no corresponding `{mission_code}.xml` file. | Log `ERROR`; send `NACK` with `requeue=false` (DLQ). | Triggers administrator alert to provision the missing database. |
| **Malformed XTCE XML** | XML contains schema errors. | Log `ERROR` on load attempt; prevent caching. | Use previous cached version if available; emit alert to alert port. |
| **Bit Offset Out-Of-Bounds** | Packet data is truncated; XTCE requires more bits. | Flag parameter as `PARAMETER_VALIDITY_INVALID` in envelope; log `WARN`. | Publish the envelope with invalid parameter flag (do not crash). |
| **Calibrator Exception** | Math overflow or spline lookup out of bounds. | Fallback to raw value; flag parameter as `PARAMETER_VALIDITY_INVALID`. | Log `WARN` and continue processing. |
| **RabbitMQ Disconnection** | Broker connection drops. | Loop and retry connection using exponential backoff. | Process block and hold consumer thread until connection is restored. |

---

## 4. Logging & Metrics

### 4.1 Structured Logging (JSON)
Logs are emitted via `tracing` structured logging:
- `info`: "Loaded XTCE database for mission {mission_code} in {duration_ms}ms"
- `info`: "Processed envelope {envelope_id} in {duration_us}us: parameters={param_count}"
- `warn`: "Parameter {param_name} decommutation out of bounds: length={len_bytes}"
- `error`: "Failed to load XTCE database for mission {mission_code}: {error_details}"

### 4.2 Prometheus Metrics
The service exposes an HTTP endpoint at `/metrics` (Port `8084`):
- `xtce_packets_processed_total`: Counter tracking processed packets. Labels: `mission`, `satellite`, `apid`, `status` (`success`, `failed`).
- `xtce_processing_latency_seconds`: Histogram tracking parsing latency.
- `xtce_db_cache_size`: Gauge tracking the count of loaded XTCE databases in memory.
- `xtce_db_load_errors_total`: Counter tracking database load failures. Labels: `mission`.

---

## 5. Testing Strategy

### 5.1 Unit Testing (Domain Core)
- **Decommutation Parser**: Test extraction of various types (unaligned signed integers, floats, string fields) from hardcoded byte buffers.
- **Calibrator Logic**:
  - Test polynomial evaluation: $y = 0.0 + 2.0x$ and $y = 1.0 + 0.5x + 3.0x^2$.
  - Test spline interpolators with linear extrapolation behavior.
  - Test state enumerations with valid and default fallbacks.
- **Registry Test**: Mock disk paths and load synthetic XML snippets.

### 5.2 Integration Testing (Ports & Adapters)
- **Lapin Driver Mock**: Test consumer startup and message acknowledge routines.
- **Serialization Test**: End-to-end Protobuf serialize and deserialize validation.
- **Registry Thread Safety**: Concurrent threads reading/writing the `XtceRegistry`.

### 5.3 End-to-End Testing (Replay Pipeline)
- Run Replay Simulator playing a binary telemetry log -> Telemetry Gateway -> CCSDS Decoder -> Mission Identification -> XTCE Decoder.
- Verify that final messages on `telemetry.engineering` contain the expected calibrated parameter names and values.

---

## 6. Sprint-Based Implementation Roadmap

```
                          IMPLEMENTATION ROADMAP
 
   Phase 1: Domain & Parsing     Phase 2: Adapters & Config      Phase 3: Integration & QA
   ┌───────────────────────┐     ┌────────────────────────┐     ┌────────────────────────┐
   │ - XtceDb Domain Model │     │ - AppConfig            │     │ - XtceOrchestrator     │
   │ - XML Parser Core     ├────>│ - RabbitMqConsumer     ├────>│ - E2E Pipeline Testing │
   │ - Decommutation Engine│     │ - RabbitMqPublisher    │     │ - Metrics Endpoint     │
   │ - Calibrator Math     │     │ - Logging & Alert ports│     │ - Performance Tuning   │
   └───────────────────────┘     └────────────────────────┘     └────────────────────────┘
```

### 6.1 Phase 1: Domain Core & Parsing (Sprint 1)
- **Goal**: Implement all memory-safe, framework-free domain logic.
- **Deliverables**:
  - `src/domain/models.rs`: Domain models representing containers and parameters.
  - `src/domain/decommutation.rs`: Bit unpacking routines.
  - `src/domain/calibration.rs`: Calibrator mathematical logic.
  - `src/domain/registry.rs`: Thread-safe XML registry loader and validator.
  - Full suite of unit tests validating bit parsing and calibration math.

### 6.2 Phase 2: Ports, Adapters, and Configuration (Sprint 2)
- **Goal**: Setup configurations, compile protobuf definitions, and wire up message bus.
- **Deliverables**:
  - `src/config.rs`: Environment configuration loader.
  - `src/proto.rs`: Protobuf schema files compiled using `prost-build`.
  - `src/ports/`: Interfaces for consumer, publisher, and alert logging.
  - `src/adapters/inbound/rabbitmq_consumer.rs`: Async AMQP message ingestion.
  - `src/adapters/outbound/rabbitmq_publisher.rs`: Downstream AMQP publication.
  - `src/adapters/outbound/console_sink.rs`: Console execution output logger.

### 6.3 Phase 3: Orchestration, Metrics, and E2E Integration (Sprint 3)
- **Goal**: Implement Composition Root, orchestrate pipeline stages, and perform validation.
- **Deliverables**:
  - `src/application/orchestrator.rs`: Wiring decommutation, calibration, and adapters.
  - `src/main.rs`: Compose service modules and startup async loop.
  - Metrics web endpoint setup using `warp` or `axum` hosting `/metrics`.
  - Smoke tests replaying simulated telemetry packets through the entire pipeline.

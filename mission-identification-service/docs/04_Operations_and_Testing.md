# Mission Identification Service — Operations and Testing Document

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-MIS-OPS-004                         |
| **Version**        | 1.0.0                                    |
| **Date**           | 2026-07-09                               |
| **Status**         | PROPOSED                                 |

---

## 1. Configuration Specification

The service loads all configuration settings from environment variables on startup. If mandatory parameters are missing, the service fails fast.

| Environment Variable | Default Value | Required | Description |
|----------------------|---------------|----------|-------------|
| `AMQP_URL` | *(None)* | **Yes** | RabbitMQ broker connection URI (e.g. `amqp://guest:guest@localhost:5672/%2f`) |
| `REGISTRY_FILE_PATH` | `configs/registry.yaml` | No | Path to the YAML rule configuration registry file |
| `SOURCE_EXCHANGE` | `telemetry.decoded` | No | Exchange from which packets are consumed |
| `SOURCE_QUEUE` | `mission.identify` | No | Name of the durable queue bound to `SOURCE_EXCHANGE` |
| `SOURCE_ROUTING_KEY` | `#.decoded` | No | Binding routing key filter pattern |
| `DESTINATION_EXCHANGE` | `telemetry.identified` | No | Target exchange for publishing enriched packets |
| `PREFETCH_COUNT` | `50` | No | AMQP QoS prefetch count (number of concurrent unacked messages) |
| `PUBLISH_TIMEOUT_MS` | `5000` | No | Timeout in milliseconds waiting for publisher confirms |
| `RETRY_MAX_ATTEMPTS` | `5` | No | Maximum retry attempts for transient RabbitMQ publishing errors |
| `RUST_LOG` | `info` | No | Log verbosity filter (`debug`, `info`, `warn`, `error`) |
| `METRICS_PORT` | `8083` | No | Port on which Prometheus scrapable HTTP endpoint is hosted |

---

## 2. Logging & Metrics

### 2.1 Prometheus Metrics
The service exposes standard Prometheus metrics on `http://0.0.0.0:${METRICS_PORT}/metrics`:

| Metric Name | Type | Labels | Description |
|-------------|------|--------|-------------|
| `must_mis_packets_processed_total` | Counter | `status` (success/failed) | Total packets processed by the identification engine. |
| `must_mis_packets_unidentified_total`| Counter | `source_id`, `apid` | Packets that failed rule lookup. |
| `must_mis_lookup_duration_seconds` | Histogram | None | Time taken to execute rule lookup. |
| `must_mis_active_rules` | Gauge | None | Number of active rules parsed from registry. |
| `must_mis_publish_retries_total` | Counter | None | Total count of outbound publishing retries. |

### 2.2 Structured Logs
All stdout logs are structured in JSON format. Critical context variables injected on telemetry processing:
- `envelope_id`: UUID of envelope.
- `source_id`: Source stream identifier.
- `apid`: CCSDS APID.
- `resolved_mission`: Mission code if successfully matched.
- `resolved_satellite`: Satellite ID if successfully matched.

---

## 3. Testing Strategy

```
┌────────────────────────────────────────────────────────┐
│                   Testing Strategy                     │
│                                                        │
│   ┌────────────────────────────────────────────────┐   │
│   │               Integration Tests                │   │
│   │ - Spin up RabbitMQ Docker container            │   │
│   │ - Publish raw .decoded, expect .identified      │   │
│   └──────────────────────┬─────────────────────────┘   │
│                          │                             │
│             ┌────────────▼──────────────┐              │
│             │        Unit Tests         │              │
│             │ - Registry parser checks  │              │
│             │ - Rule specificity checks │              │
│             └────────────┬──────────────┘              │
│                          │                             │
│            ┌─────────────▼───────────────┐             │
│            │      Performance Tests      │             │
│            │ - Micro-benchmarks (Lookup) │             │
│            │ - Max throughput stress     │             │
│            └─────────────────────────────┘             │
└────────────────────────────────────────────────────────┘
```

### 3.1 Unit Testing Matrix
- **UT-101**: Validate loading invalid YAML configurations returns configuration error.
- **UT-102**: Ensure lookup matching resolves correctly under specific rule matching (source + APID takes precedence over APID-only).
- **UT-103**: Test behavior of empty or unregistered APIDs (should return `LookupError::Unidentified`).
- **UT-104**: Test duplicate/ambiguous rules resolution.

### 3.2 Integration Testing Matrix
- **IT-201**: Verify end-to-end packet ingestion, lookup matching, mutation, and egress to `telemetry.identified`.
- **IT-202**: Verify AMQP publisher confirms and retry logic under transient broker drops.
- **IT-203**: Validate Dead Letter Queue (DLQ) routing when lookup matching fails.

---

## 4. Development Sprints Roadmap

Following the SDD and Hexagonal lifecycle, development is split into 4 logical sprints:

### Sprint 1: Domain Logic and Registry Engine (2 Days)
- Define Protobuf structures and compile definitions.
- Implement the `MissionRegistry` configuration parser.
- Implement the `RuleLookupEngine` matching engine.
- Write unit tests for all domain structures (`cargo test`).

### Sprint 2: Hexagonal Ports and Mock Drivers (2 Days)
- Define inbound ports (`EnvelopeConsumer`, `DeliveryAcker`) and outbound ports (`IdentifiedPublisher`).
- Wire ports to the orchestrator `IdentificationOrchestrator`.
- Create mock implementations of ports to run core logic tests.

### Sprint 3: AMQP Adapters Integration (2 Days)
- Implement `RabbitMqConsumer` using `lapin` asynchronous client.
- Implement `RabbitMqPublisher` with publisher confirms enabled.
- Connect telemetry stream with logging and health configurations.

### Sprint 4: Performance Verification & End-to-End Tests (2 Days)
- Implement Prometheus metrics endpoint.
- Create container multi-stage Dockerfile and deployment manifest templates.
- Run integration tests against live RabbitMQ container.
- Perform high-throughput benchmarks (target > 150,000 packets/sec).

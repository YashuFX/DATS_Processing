# Engineering Conversion Service — Operations, Testing, and Roadmap

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-ECS-OPS-004                        |
| **Version**        | 1.0.0                                    |
| **Date**           | 2026-07-10                               |
| **Status**         | PROPOSED                                 |

---

## 1. Configuration Specification

The service loads all configuration variables from environment variables at startup. If a mandatory variable is missing, the service immediately terminates (fail-fast).

| Environment Variable | Required | Default Value | Description |
|----------------------|----------|---------------|-------------|
| `AMQP_URL`           | **Yes**  | N/A           | AMQP Broker connection URI. |
| `SOURCE_EXCHANGE`    | No       | `telemetry.engineering` | Exchange to consume decommutated envelopes from. |
| `SOURCE_QUEUE`       | No       | `engineering.convert` | Queue name to bind and consume. |
| `SOURCE_ROUTING_KEY` | No       | `#.decommutated`| Binding key pattern to receive decommutated telemetry. |
| `CONSUMER_TAG`       | No       | `ecs-converter-1` | Unique consumer identifier registered with RabbitMQ. |
| `PREFETCH_COUNT`     | No       | `50`          | QoS prefetch window size. |
| `DESTINATION_EXCHANGE` | No     | `telemetry.engineering` | Exchange to publish computed engineering envelopes to. |
| `DERIVED_DB_DIR`     | No       | `/etc/must/derived` | Directory containing mission YAML files for formulas. |
| `PUBLISH_TIMEOUT_MS` | No       | `5000`        | Timeout waiting for downstream publish confirmations. |
| `RETRY_MAX_ATTEMPTS` | No       | `5`           | Maximum retry attempts for transient publish failures. |

---

## 2. Validation Rules

To safeguard system integrity, validation checks are performed during configuration loading and runtime processing:

### 2.1 Configuration Validation (Load Time)
When compiling a derived parameter configuration YAML file for a mission:
* **YAML Well-Formedness**: The file must be valid YAML.
* **Expression Syntactic Validity**: All math and logical expressions must compile successfully through the evaluation engine (detecting syntax errors or unsupported operators early).
* **Direct Cycle Detection**: No derived parameter may use another derived parameter as an input in a way that forms a cyclic dependency. We construct a directed dependency graph of derived parameters and verify it is a Directed Acyclic Graph (DAG) using a topological sort.
* **Duplicate Definitions**: Parameter names defined in the `derived_parameters` list must be unique.

### 2.2 Packet Validation (Runtime)
When processing an incoming `TelemetryEnvelope`:
* **Mission Check**: The envelope must contain a valid `mission.mission_code` string.
* **Input Resolution**: The engine checks if the required input parameters listed in the formula definition exist inside the envelope's parameters list. If an input is missing, the calculation is marked invalid.
* **Type Safety & Bounds**: Division by zero and overflow conditions (e.g., $x^y$ exceeding float storage) are intercepted.

---

## 3. Error Handling Strategy

| Error Scenario | Root Cause | System Response | Recovery Action |
|----------------|------------|-----------------|-----------------|
| **Deserialization Failure** | Corrupt envelope payload from RabbitMQ. | Log error; send `NACK` with `requeue=false` to RabbitMQ. | Envelope is routed to the Dead Letter Queue (`must.dlx`). |
| **Missing YAML File** | Mission code has no corresponding `{mission_code}.yaml` file. | Log `ERROR`; send `NACK` with `requeue=false` (DLQ). | Triggers administrator alert to provision the missing formula database. |
| **Malformed YAML File** | YAML contains syntax or configuration errors. | Log `ERROR` on load attempt; prevent caching. | Use previous cached version if available; emit alert to alert port. |
| **Missing Input Parameter** | Envelope lacks one of the parameters required by the formula. | Flag parameter as `PARAMETER_VALIDITY_INVALID` in envelope; log `WARN`. | Publish the envelope with invalid parameter flag (do not crash). |
| **Divide by Zero / Math Error**| Calculation evaluates to infinity or NaN. | Fallback to default/NaN value; set validity flag to `PARAMETER_VALIDITY_INVALID`. | Log `WARN` and continue processing. |
| **RabbitMQ Disconnection** | Broker connection drops. | Loop and retry connection using exponential backoff. | Process block and hold consumer thread until connection is restored. |

---

## 4. Logging & Metrics

### 4.1 Structured Logging (JSON)
Logs are written to stdout using the `tracing` framework:
* `info`: "Loaded derived parameter configurations for mission {mission_code} with {param_count} formulas in {duration_ms}ms"
* `info`: "Enriched envelope {envelope_id} in {duration_us}us: derived={derived_count}"
* `warn`: "Formula {derived_name} failed: missing input parameter {input_name} in envelope {envelope_id}"
* `error`: "Failed to load formula file for mission {mission_code}: {error_details}"

### 4.2 Prometheus Metrics
The service exposes a `/metrics` HTTP endpoint on port `8085`:
* `ecs_packets_processed_total`: Counter tracking processed envelopes. Labels: `mission`, `satellite`, `apid`, `status` (`success`, `failed`).
* `ecs_processing_latency_seconds`: Histogram tracking processing latency.
* `ecs_db_cache_size`: Gauge tracking the count of loaded configuration databases in memory.
* `ecs_db_load_errors_total`: Counter tracking configuration loading errors. Labels: `mission`.
* `ecs_formula_failures_total`: Counter tracking formula evaluation failures. Labels: `mission`, `derived_parameter`.

---

## 5. Testing Strategy

### 5.1 Unit Testing (Domain Core)
* **Expression Parser**: Test evaluation of mathematical functions, algebraic terms, logical conditions, and ternary branching using mock parameter sets.
* **Dependency Validation**: Test cycle detection logic with synthetic DAGs and cyclic configurations, ensuring cycles are correctly rejected.
* **Type Safety Tests**: Validate calculations with mixed inputs (floats, integers, booleans, strings) and verify correct type conversions.

### 5.2 Integration Testing (Ports & Adapters)
* **Registry Thread Safety**: Assert concurrent threads can safely perform reads and trigger cache loads without race conditions.
* **Lapin Consumer/Publisher Mocking**: Test consumer startup, acknowledgement mechanisms, and publisher confirmations.
* **Protobuf Compatibility**: Validate envelope serialization and deserialization against the shared Protobuf contract.

### 5.3 End-to-End Testing (Replay Pipeline)
* Deploy Replay Simulator -> Gateway -> CCSDS -> Mission ID -> XTCE -> Conversion Service.
* Replay a packet containing raw telemetry. Ensure the final published envelope on `telemetry.engineering` contains both decommutated XTCE parameters and derived parameters computed by the Conversion Service.

---

## 6. Implementation Roadmap (3-Phase Plan)

To minimize integration risk and ensure progressive stability, the service is built in three milestones. Each phase delivers a functional, testable subset of the service:

```
                            IMPLEMENTATION ROADMAP (3-PHASE PLAN)

     Phase 1: Domain Core & Math Engine      Phase 2: Wiring & Orchestration       Phase 3: Production Readiness & E2E
     ┌────────────────────────────────┐      ┌─────────────────────────────┐      ┌─────────────────────────────┐
     │ - AST expression evaluator     │      │ - RabbitMQ Inbound Consumer │      │ - Prometheus metrics (8085) │
     │ - Config registry & Cache      ├─────>│ - RabbitMQ Publisher/Conf   ├─────>│ - Liveness/Readiness probes │
     │ - Topological DAG Validation   │      │ - ConversionOrchestrator    │      │ - Multi-stage Docker build  │
     │ - 100% unit test coverage      │      │ - Configuration loader      │      │ - Pipeline E2E smoke tests  │
     └────────────────────────────────┘      └─────────────────────────────┘      └─────────────────────────────┘
```

### Phase 1: Domain Core & Computation Engine (The Heart)
* **Goal**: Implement all memory-safe, framework-free business logic, formula configurations, expression parser, and unit testing. This ensures the calculation core is completely decoupled and fully validated in isolation.
* **Deliverables**:
  * `src/domain/models.rs`: Structs for `DerivedDb`, `DerivedParameterDefinition`, and `InputMapping`.
  * `src/domain/registry.rs`: YAML registry loader with strict validation (including directed acyclic graph topological sort for cycle detection).
  * `src/domain/computation.rs`: AST-based formula evaluator (using the `evalexpr` engine) mapping telemetry inputs, evaluating logic/math, and returning typed outputs.
  * `src/domain/errors.rs`: Domain error catalog (`DomainError`).
  * **Testing**: 100% test coverage on algebraic operations, logical checks, conditional/ternary statements, type casting/promotion, cycle detection, and negative edge cases (e.g. division by zero).

### Phase 2: Ports, Adapters, & Orchestration (The Wiring)
* **Goal**: Wire the pure domain logic to external components using Hexagonal Ports and implement the asynchronous use case flow.
* **Deliverables**:
  * `src/ports/inbound.rs` & `src/ports/outbound.rs`: Trait definitions for `EnvelopeConsumer`, `DeliveryAcker`, `EngineeringPublisher`, and `AlertPort`.
  * `src/adapters/inbound/rabbitmq_consumer.rs`: Consuming from `telemetry.engineering` with queue `engineering.convert` bound to routing key `#.decommutated`.
  * `src/adapters/outbound/rabbitmq_publisher.rs`: Publishing back to `telemetry.engineering` (routing key: `{mission_code}.sat{satellite_id}.{apid}.engineering`) with publisher confirmations.
  * `src/application/orchestrator.rs`: Coordinate the use case: deserialize envelope, query formula database, trigger math evaluations, append outputs, stamp stage to `PROCESSING_STAGE_ENGINEERING_CONVERTED`, and manage manual acknowledgements (`ack`/`nack`).
  * `src/config.rs`: Environment-based application configuration loader.
  * `src/main.rs`: Composition root to instantiate modules and bootstrap the asynchronous runner.
  * **Testing**: Integration tests using mock consumers, mock publishers, and mock ackers to verify correct in-memory coordination.

### Phase 3: Observability, Integration, & Production Readiness (The Release)
* **Goal**: Prepare the microservice for a containerized, production-grade cloud environment and execute full-pipeline verification.
* **Deliverables**:
  * `/metrics` endpoint: Host an HTTP server on port `8085` using `warp`/`axum` and export counters/histograms for packet throughput, processing latency, config cache size, and formula evaluation failure rates.
  * Liveness/readiness check: Health checking capabilities tracking TCP connection state to RabbitMQ.
  * Production packaging: Multi-stage non-root `Dockerfile` using `debian-slim` base image.
  * System E2E integration: Launch the entire telemetry pipeline (Replay Simulator -> Gateway -> CCSDS -> Mission ID -> XTCE -> Conversion Service) and assert derived channels flow accurately to downstream queues.


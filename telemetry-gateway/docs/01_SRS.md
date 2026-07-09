# MuST Telemetry Gateway — Software Requirements Specification

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-GW-SRS-001                          |
| **Version**        | 1.0.0-DRAFT                             |
| **Date**           | 2026-07-03                               |
| **Status**         | DRAFT — PENDING REVIEW                   |
| **Depends On**     | MUST-SIM-SRS-001, MUST-CONTRACTS-002, MUST-BUS-003 |

---

## 1. Purpose

The Telemetry Gateway is the **single entry point** for all telemetry entering the MuST platform. Every packet — whether from a Replay Simulator, live ground station receiver, or future SDR — passes through this service before reaching the internal event bus.

**Why a dedicated Gateway:**
- **Normalization:** Different sources produce packets in different formats and with different metadata completeness. The Gateway produces a uniform `TelemetryEnvelope` regardless of source.
- **Validation at the boundary:** Invalid packets are rejected before entering the bus. Without a gateway, every downstream service must independently validate every packet — N services × M validation rules = combinatorial complexity.
- **Enrichment:** The Gateway stamps mission context, ground station identity, and gateway-assigned sequence numbers. Sources should not need to know the full MuST configuration.
- **Auditability:** A single ingress point means a single audit log. Every packet that enters MuST has a Gateway timestamp.
- **Security boundary:** The Gateway is the only service that accepts connections from external sources. Internal services only consume from RabbitMQ.

**Architectural Refinement (ADR-007):**
The existing Message Bus Design (MUST-BUS-003) shows sources publishing directly to `telemetry.raw`. With the Gateway specified, the architecture refines to: Sources → Gateway → `telemetry.raw`. The Gateway is the **sole publisher** to `telemetry.raw`. This ensures no unvalidated packets reach the internal bus.

---

## 2. Scope

### 2.1 In Scope

- Accept telemetry from multiple concurrent sources via pluggable adapters
- Validate packet envelope completeness and integrity
- Enrich packets with mission context, station identity, gateway sequence
- Publish validated `TelemetryEnvelope` messages to RabbitMQ `telemetry.raw`
- Publish platform events to RabbitMQ `must.events`
- Track per-source and per-session statistics (via logs/metrics)
- gRPC Ingress API
- Prometheus metrics and health probes (via gRPC/HTTP endpoint)

### 2.2 Out of Scope (Gateway MUST NOT)

| Exclusion | Rationale |
|-----------|-----------|
| Decode CCSDS payloads | Responsibility of the CCSDS Service |
| Parse XTCE databases | Responsibility of the XTCE Service |
| Extract telemetry parameters | Responsibility of the XTCE Service |
| Store packets persistently | Responsibility of the Archive Service |
| Generate reports | Responsibility of the Dashboard Service |
| Interpret packet semantics | Gateway is transport-layer, not application-layer |

---

## 3. Functional Requirements

### 3.1 Source Ingestion

| ID      | Requirement | Priority | Rationale |
|---------|-------------|----------|-----------|
| GW-010  | SHALL accept telemetry packets from the Replay Simulator via gRPC streaming | MUST | Primary source during development |
| GW-011  | SHALL define a `TelemetrySource` abstraction in the inbound ports | MUST | Future sources (TCP, UDP, SDR) require zero gateway logic changes |
| GW-012  | SHALL support multiple concurrent source connections | MUST | Multi-station tracking produces simultaneous feeds |
| GW-013  | SHALL assign a unique session ID to each source connection | MUST | Enables per-session statistics and lifecycle tracking |
| GW-014  | SHALL validate source configurations statically from startup configuration | MUST | Prevents unidentified sources from injecting packets (dynamic registration deferred) |
| GW-015  | (DEFERRED) SHALL support dynamic source registration and deregistration at runtime | DEFERRED | Deferred to v2 |
| GW-016  | SHALL detect source disconnection and emit a SourceDisconnected event | MUST | Operators need immediate visibility into source failures |
| GW-017  | SHALL support future TCP, UDP, Serial, GNU Radio, and SDR sources without modifying business logic | MUST | TelemetrySource abstraction is the extension point |

### 3.2 Validation

| ID      | Requirement | Priority | Rationale |
|---------|-------------|----------|-----------|
| GW-020  | SHALL validate that every incoming packet has a non-empty payload | MUST | Empty packets waste bus capacity and confuse downstream |
| GW-021  | SHALL validate that envelope timestamps are present and non-zero | MUST | Timestamp-less packets cannot be ordered or correlated |
| GW-022  | SHALL detect and flag duplicate sequence numbers from the same source | MUST | Duplicates corrupt sequence gap analysis downstream |
| GW-023  | SHALL detect and flag sequence gaps from the same source | MUST | Gaps indicate packet loss at the source |
| GW-024  | SHALL reject packets from unregistered sources | MUST | Security: prevent unauthorized telemetry injection |
| GW-025  | SHALL reject packets from unknown replay sessions | MUST | Orphan packets from terminated sessions must not enter the bus |
| GW-026  | SHALL validate envelope_id is present and unique (within a window) | SHOULD | Deduplication across source reconnections |
| GW-027  | SHALL NEVER validate CCSDS payload contents | MUST | Gateway is envelope-only. CCSDS validation is downstream. |

### 3.3 Enrichment

| ID      | Requirement | Priority | Rationale |
|---------|-------------|----------|-----------|
| GW-030  | SHALL stamp every packet with a gateway receive timestamp (nanosecond, monotonic-derived) | MUST | Authoritative ingestion time for the platform |
| GW-031  | SHALL assign mission context (MissionIdentifier) from source registration config | MUST | Sources may not know mission details; Gateway resolves from config |
| GW-032  | SHALL assign satellite context (SatelliteIdentifier) from source registration config | MUST | Same rationale as mission context |
| GW-033  | SHALL assign ground station context (GroundStationIdentifier) from source registration config | MUST | Multi-station tracking requires station attribution |
| GW-034  | SHALL assign a gateway-scoped monotonic sequence number to every published envelope | MUST | Global ordering within the gateway for downstream gap detection |
| GW-035  | SHALL set ProcessingStage to PROCESSING_STAGE_RAW on every published envelope | MUST | Routing key construction requires stage field |
| GW-036  | SHALL set publish_timestamp immediately before publishing to RabbitMQ | MUST | Measures gateway processing latency (receive_ts vs publish_ts) |
| GW-037  | SHALL populate QualityIndicator based on validation results | MUST | Downstream services use quality flags for filtering |

### 3.4 Publishing

| ID      | Requirement | Priority | Rationale |
|---------|-------------|----------|-----------|
| GW-040  | SHALL publish validated envelopes to RabbitMQ `telemetry.raw` exchange | MUST | Single publisher to telemetry.raw (ADR-007) |
| GW-041  | SHALL construct routing keys per the platform schema: `{mission}.{satellite}.{apid}.raw` | MUST | Enables selective consumption by downstream services |
| GW-042  | SHALL set all required AMQP message properties per MUST-BUS-003 Section 7 | MUST | Wire-level interoperability |
| GW-043  | SHALL publish platform events to RabbitMQ `must.events` exchange | MUST | Lifecycle and error events for monitoring |
| GW-044  | SHALL handle RabbitMQ connection failures with automatic reconnection and exponential backoff | MUST | RabbitMQ restarts must not crash the Gateway |
| GW-045  | SHALL buffer packets during brief RabbitMQ disconnections (up to configurable limit) | SHOULD | Prevents packet loss during transient outages |
| GW-046  | SHALL reject new packets when buffer is full, emitting QueueFull event | MUST | Prevents OOM. Backpressure propagates to sources |

### 3.5 Session Management

| ID      | Requirement | Priority | Rationale |
|---------|-------------|----------|-----------|
| GW-050  | SHALL maintain per-session statistics (packets received, published, rejected, errors) | MUST | Operational visibility |
| GW-051  | SHALL detect session completion (source signals EOF) and emit SessionFinished | MUST | Session lifecycle tracking |
| GW-052  | (DEFERRED) SHALL support operator-initiated session stop via REST API | DEFERRED | Deferred to v2 |
| GW-053  | (DEFERRED) SHALL support querying all active sessions via REST API | DEFERRED | Deferred to v2 |

### 3.6 API

| ID      | Requirement | Priority | Rationale |
|---------|-------------|----------|-----------|
| GW-060  | (DEFERRED) SHALL expose REST API for source registration, status, and statistics | DEFERRED | Deferred to v2 |
| GW-061  | (DEFERRED) SHALL expose WebSocket endpoint for real-time status streaming | DEFERRED | Deferred to v2 |
| GW-062  | SHALL expose Prometheus metrics | MUST | Standard observability |
| GW-063  | SHALL expose health endpoints | MUST | Standard health probes |

---

## 4. Non-Functional Requirements

### 4.1 Performance

| ID       | Requirement | Target | Rationale |
|----------|-------------|--------|-----------|
| GW-N010  | Packet ingestion throughput | > 150,000 pkt/s | Must exceed Replay Simulator's max output (100K at 32x) with headroom for multi-source |
| GW-N011  | Gateway processing latency (receive → publish) | P99 < 5 ms | Gateway must not be a bottleneck. Near-zero added latency. |
| GW-N012  | Source registration latency | < 100 ms | Operator responsiveness |
| GW-N013  | Concurrent source connections | >= 16 | Multi-station, multi-replay |

### 4.2 Reliability

| ID       | Requirement | Target | Rationale |
|----------|-------------|--------|-----------|
| GW-N020  | Auto-reconnect to RabbitMQ | < 5 seconds | Transient broker restarts |
| GW-N021  | Packet loss during reconnection | 0 (buffered) | At-least-once delivery to the bus |
| GW-N022  | MTBF | > 2000 hours | Gateway is single entry point; higher reliability than any individual service |
| GW-N023  | Horizontal scalability | Supported | Multiple Gateway instances behind a load balancer |

### 4.3 Observability

| ID       | Requirement | Target |
|----------|-------------|--------|
| GW-N030  | Structured JSON logs via `tracing` | — |
| GW-N031  | Prometheus metrics (defined in 06_Deployment) | — |
| GW-N032  | (DEFERRED) OpenTelemetry distributed tracing | — |
| GW-N033  | Per-source, per-session, per-mission metric labels | — |

---

## 5. Constraints

| ID      | Constraint | Rationale |
|---------|------------|-----------|
| GW-C001 | Language: Rust (2021 Edition) | Safety, performance, strict alignment with Replay Simulator |
| GW-C002 | Framework: Tonic (gRPC) | Type-safe contract generation, high throughput |
| GW-C003 | Logging: `tracing` subscriber | Structured, high-performance logging standard for Rust |
| GW-C004 | (DEFERRED) Tracing: OpenTelemetry | Deferred to v2 |
| GW-C005 | Architecture: Hexagonal (Ports & Adapters) | Domain isolation from transport frameworks |
| GW-C006 | Configuration: Config file / environment overrides | Platform standard |
| GW-C007 | Serialization: Protocol Buffers (shared contracts) | Platform standard (MUST-CONTRACTS-002) |
| GW-C008 | Message Bus: RabbitMQ (lapin) | Platform standard (ADR-001/006) |

---

## 6. Revision History

| Version | Date       | Description |
|---------|------------|-------------|
| 1.0.0   | 2026-07-03 | Initial draft |

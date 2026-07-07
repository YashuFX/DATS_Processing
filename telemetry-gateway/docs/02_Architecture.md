# MuST Telemetry Gateway — Architecture Document

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-GW-ARCH-002                         |
| **Version**        | 1.0.0-DRAFT                             |
| **Date**           | 2026-07-03                               |
| **Status**         | DRAFT — PENDING REVIEW                   |

---

## 1. Architectural Position

The Gateway is the **security and normalization boundary** between external telemetry sources and the internal MuST event bus.

```
┌──────────────────────────────────────────────────────────────────────────┐
│                        EXTERNAL (Untrusted)                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                   │
│  │   Replay     │  │   Live       │  │   SDR        │                   │
│  │   Simulator  │  │   Receiver   │  │   (future)   │                   │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘                   │
│         │                 │                 │                             │
│         └─────────────────┼─────────────────┘                            │
│                           │  gRPC / TCP / UDP                            │
├───────────────────────────┼──────────────────────────────────────────────┤
│                    ┌──────▼──────┐    TELEMETRY GATEWAY                  │
│                    │  Validate   │                                       │
│                    │  Enrich     │                                       │
│                    │  Publish    │                                       │
│                    └──────┬──────┘                                       │
├───────────────────────────┼──────────────────────────────────────────────┤
│                           │  AMQP                                        │
│                    ┌──────▼──────┐    INTERNAL (Trusted)                 │
│                    │  RabbitMQ   │                                       │
│                    │  telemetry  │                                       │
│                    │  .raw       │                                       │
│                    └─────────────┘                                       │
│         ┌─────────────────┼─────────────────┐                            │
│         ▼                 ▼                 ▼                            │
│  ┌────────────┐  ┌──────────────┐  ┌──────────────┐                    │
│  │ CCSDS Svc  │  │ Archive Svc  │  │ Other Svc    │                    │
│  └────────────┘  └──────────────┘  └──────────────┘                    │
└──────────────────────────────────────────────────────────────────────────┘
```

**Key principle:** Nothing enters `telemetry.raw` without passing through the Gateway. This is enforced by RabbitMQ permissions — only the Gateway's AMQP user has publish rights to `telemetry.raw`.

---

## 2. Framework Decision: Axum

| Candidate | Strengths | Weaknesses | Verdict |
|-----------|-----------|------------|---------|
| **Axum** | Native integration with Tokio runtime, Tower middleware ecosystem, type-safe routing, excellent integration with Tonic gRPC and Prometheus/OTel tracing | Learning curve for Rust-specific traits/types | **Selected** |
| Actix-web | Extremely fast, mature actor-like framework | Relies on its own tokio wrapper/runtime versions, harder to share custom middleware with Tonic/Tower | Rejected |

**Why Axum wins:** The Gateway's REST API is low-volume (operator control). The hot path is gRPC ingestion → RabbitMQ publishing, where the HTTP framework is irrelevant. Choosing Actix-web at the cost of unified Tokio/Tower middleware compatibility would be an engineering misjudgment. Axum's compatibility with Tower middleware, Tonic gRPC, and standard Rust async tooling aligns perfectly with the rest of the MuST platform.

---

## 3. Hexagonal Architecture

### 3.1 Layer Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                    DRIVING ADAPTERS (Inbound)                       │
│  ┌───────────────┐  ┌──────────────┐  ┌──────────────────────────┐ │
│  │ gRPC Receiver │  │ REST API     │  │ WebSocket Status         │ │
│  │ (Replay)      │  │ (Axum)       │  │                          │ │
│  └───────┬───────┘  └──────┬───────┘  └───────────┬──────────────┘ │
│          │                 │                      │                 │
│          └─────────────────┼──────────────────────┘                 │
│                            │                                        │
│                    ┌───────▼───────┐                                │
│                    │    PORTS      │ (IngestPort, ControlPort)      │
│                    └───────┬───────┘                                │
├────────────────────────────┼────────────────────────────────────────┤
│                     APPLICATION LAYER                                │
│  ┌─────────────────────────▼─────────────────────────────────┐      │
│  │               Ingestion Orchestrator                       │      │
│  │  ┌──────────────┐ ┌──────────────┐ ┌────────────────────┐│      │
│  │  │  Validator   │ │  Enricher    │ │ Session Manager    ││      │
│  │  └──────────────┘ └──────────────┘ └────────────────────┘│      │
│  │  ┌──────────────┐ ┌──────────────┐                       │      │
│  │  │Source Registry│ │ Statistics   │                       │      │
│  │  └──────────────┘ └──────────────┘                       │      │
│  └─────────────────────────┬─────────────────────────────────┘      │
│                            │                                        │
│                    ┌───────▼───────┐                                │
│                    │    PORTS      │ (PublishPort, EventPort)       │
│                    └───────┬───────┘                                │
├────────────────────────────┼────────────────────────────────────────┤
│                    DRIVEN ADAPTERS (Outbound)                        │
│  ┌───────────────┐ ┌───────────────┐ ┌────────────┐                  │
│  │ RabbitMQ      │ │ RabbitMQ      │ │ OTel       │                  │
│  │ Publisher     │ │ Events        │ │ (Tracing)  │                  │
│  │ Holy Grail    │ │ Gateway       │ │            │                  │
│  └───────────────┘ └───────────────┘ └────────────┘                  │
└─────────────────────────────────────────────────────────────────────┘
```

### 3.2 Component Responsibilities

#### Ingestion Orchestrator
**Purpose:** Central coordinator. Receives validated packets, enriches them, publishes to the bus.

**Why it exists:** Identical rationale to the Replay Simulator's Orchestrator — a single coordination point prevents race conditions and centralizes state management.

#### Validator
**Purpose:** Validates incoming packet envelopes against the rules in GW-020 through GW-027.

#### Enricher
**Purpose:** Populates envelope fields that the source may not have set.

**Why the Gateway overwrites `receive_timestamp`:** The Gateway is the authoritative clock for "when did MuST receive this packet?" The source's timestamp is preserved in `original_timestamp`. This separation enables latency analysis: `receive_timestamp - original_timestamp = source-to-gateway latency`.

#### Source Registry
**Purpose:** Maintains the catalog of registered sources with their configuration.

**Registration record:**
```rust
struct SourceRegistration {
    source_id:      String,    // UUID assigned by Gateway
    source_type:    SourceType, // REPLAY, TCP, UDP, etc.
    source_name:    String,    // Human-readable
    mission:        MissionIdentifier,
    satellite:      SatelliteIdentifier,
    station:        GroundStationIdentifier,
    registered_at:  DateTime<Utc>,
    status:         SourceStatus,
}
```

#### Session Manager
**Purpose:** Tracks the lifecycle of replay/reception sessions.

---

## 4. Port Definitions (Interfaces)

### 4.1 IngestPort (Driving — Inbound)

#[async_trait]
pub trait IngestPort: Send + Sync {
    async fn on_packet_received(&self, envelope: TelemetryEnvelope) -> Result<(), GatewayError>;
    async fn on_source_connected(&self, source_id: String) -> Result<(), GatewayError>;
    async fn on_source_disconnected(&self, source_id: String) -> Result<(), GatewayError>;
    async fn on_session_eof(&self, session_id: String) -> Result<(), GatewayError>;
}

### 4.2 ControlPort (Driving — Inbound)

```rust
#[async_trait]
pub trait ControlPort: Send + Sync {
    async fn register_source(&self, req: RegisterSourceRequest) -> Result<RegisterSourceResponse, GatewayError>;
    async fn unregister_source(&self, source_id: String) -> Result<(), GatewayError>;
    async fn stop_session(&self, session_id: String) -> Result<(), GatewayError>;
    async fn get_status(&self) -> Result<GatewayStatus, GatewayError>;
    async fn get_statistics(&self) -> Result<GatewayStatistics, GatewayError>;
    async fn get_sessions(&self) -> Result<Vec<Session>, GatewayError>;
}
```

### 4.3 PublishPort (Driven — Outbound)

```rust
#[async_trait]
pub trait PublishPort: Send + Sync {
    async fn publish(&self, envelope: TelemetryEnvelope, routing_key: &str) -> Result<(), GatewayError>;
    fn is_connected(&self) -> bool;
    fn buffer_depth(&self) -> usize;
}
```

### 4.4 EventPort (Driven — Outbound)

```rust
#[async_trait]
pub trait EventPort: Send + Sync {
    async fn emit(&self, event: PlatformEvent) -> Result<(), GatewayError>;
}
```

---

## 5. System Boundary: Replay Simulator vs Telemetry Gateway

To eliminate architectural ambiguity, the responsibilities are strictly separated at the gRPC stream boundary:

```
┌───────────────────────────────────────┐
│           Replay Simulator            │
├───────────────────────────────────────┤
│ Responsibilities:                     │
│ • Replay timing                       │
│ • Replay scheduling                   │
│ • Envelope creation (initial)         │
│ • gRPC streaming client               │
└──────────────────┬────────────────────┘
                   │
                   │ gRPC Stream (Tonic)
                   │
┌──────────────────▼────────────────────┐
│           Telemetry Gateway           │
├───────────────────────────────────────┤
│ Responsibilities:                     │
│ • Receive stream (gRPC server)        │
│ • Validate                            │
│ • Normalize                           │
│ • Enrich                              │
│ • Route (Generate Routing Keys)       │
│ • Publish to RabbitMQ exchange        │
└───────────────────────────────────────┘
```

---

## 6. Packet Flow Pipeline (Separated Logic)

To simplify testing and separate concerns, the ingestion pipeline splits processing into five isolated, sequential stages:

```
                gRPC Ingress Stream
                        │
                        ▼
            ┌───────────────────────┐
            │       Validator       │ ──[Invalid]──> Drop (Emit event)
            └───────────┬───────────┘
                        │ [Valid]
                        ▼
            ┌───────────────────────┐
            │      Normalizer       │ (Convert source packet format to internal model)
            └───────────┬───────────┘
                        │
                        ▼
            ┌───────────────────────┐
            │       Enricher        │ (Add Gateway UUID, receive stamp, station context)
            └───────────┬───────────┘
                        │
                        ▼
            ┌───────────────────────┐
            │        Router         │ (Build routing key: mission.sat.apid.raw)
            └───────────┬───────────┘
                        │
                        ▼
            ┌───────────────────────┐
            │       Publisher       │ ──> RabbitMQ telemetry.raw
            └───────────────────────┘
```

### Stage Responsibilities

1. **Validator**:
   * **Scope**: Pure boolean check: Is the packet valid and allowed to enter the pipeline?
   * **Checks**: Non-empty payload, original timestamp > 0, source registered, session active.
   * **Rule**: Does not alter the packet. Only returns `true` or `false` (with rejection reason).

2. **Normalizer**:
   * **Scope**: Converts source-specific replay envelopes and packets into a canonical, standardized internal `TelemetryEnvelope`.
   * **Reasoning**: Different playback files, missions, or network clients might send slightly different payloads. Normalization standardizes all headers and payloads into one model.

3. **Enricher**:
   * **Scope**: Adds gateway-authoritative tracking data.
   * **Actions**: Overwrites `receive_timestamp` with the Gateway's system clock, stamps a unique Gateway `envelope_id` (UUID), and inserts missing ground station/mission identifiers from the registry. No validation or routing decisions are made here.

4. **Router**:
   * **Scope**: Isolates RabbitMQ-specific routing key logic from the rest of the application.
   * **Action**: Builds the standard routing key `{mission_code}.sat{satellite_id}.{apid}.raw` based on packet headers and registration context.

5. **Publisher**:
   * **Scope**: Dispatches the canonical, enriched envelope with its routing key to the RabbitMQ bus.

---

## 7. Concurrency Model

**Why channel-based pipeline:** Tokio mpsc channels provide natural async backpressure. When the publish channel is full, workers block, which causes the ingestion channel to fill, which causes the Tonic gRPC adapter to exert backpressure on the streaming source.

---

## 8. RabbitMQ Integration — Gateway-Specific

### 8.1 Connection Management

```rust
pub struct RabbitMqPublisher {
    connection: lapin::Connection, // Shared connection
    channel_pool: deadpool::Pool<lapin::Channel>, // Channel pool for concurrent publishing
}
```

**Why connection pool:**
- AMQP connections are multiplexed via Lapin.
- Channel pool (using Deadpool) enables parallel publishing without connection overhead.
- Publisher confirms are handled asynchronously per message publish future, ensuring at-least-once delivery.

### 8.2 Delivery Guarantee

**At-least-once delivery.** Not exactly-once.

**Why:** Exactly-once across distributed systems (Gateway → RabbitMQ → Consumer) requires two-phase commit or idempotent consumers. Two-phase commit destroys throughput. Instead:
1. Gateway publishes with publisher confirms.
2. If confirm is negative or times out, Gateway retries.
3. Downstream consumers use `envelope_id` for idempotent processing (deduplication).

This is the standard pattern used by Kafka, AWS SQS, and Google Pub/Sub.

### 8.3 Gateway-Specific RabbitMQ Topology

| Element | Name | Purpose |
|---------|------|---------|
| Exchange | `telemetry.raw` | Primary telemetry fan-out |
| Exchange | `must.events` | Platform events |
| Exchange | `must.dlx` | Dead letter exchange |
| Queue | `gateway.retry` | Failed publishes awaiting retry |
| Queue | `gateway.dlq` | Permanently failed packets |

**Retry strategy:**
```
Publish attempt 1: immediate
  ↓ failure
Publish attempt 2: 100ms backoff
  ↓ failure
Publish attempt 3: 500ms backoff
  ↓ failure
→ Dead letter to gateway.dlq
→ Emit RetryExhausted event
→ Log dead letter drop event
```

### 8.4 Ordering Guarantee

Packets from the **same source** are published in order. Packets from **different sources** have no ordering guarantee.

**How:** Each source's packets flow through a dedicated channel partition (source_id → channel assignment via consistent hashing). A single channel preserves AMQP ordering.

---

## 9. Project Structure

```
telemetry-gateway/
├── Cargo.toml                       # Build & Dependency configuration
├── build.rs                         # Tonic protobuf compilation script
├── src/
│   ├── main.rs                      # Entry point: config, DI, server startup
│   ├── domain/                      # Pure domain logic (no I/O, no frameworks)
│   │   ├── mod.rs
│   │   ├── models.rs                # TelemetryEnvelope, Session, SourceRegistration
│   │   ├── validator.rs             # Packet validation logic
│   │   ├── enricher.rs              # Envelope enrichment logic
│   │   ├── events.rs                # Domain event types
│   │   └── errors.rs                # Domain error types
│   │
│   ├── application/                 # Use cases / orchestration
│   │   ├── mod.rs
│   │   ├── orchestrator.rs          # Ingestion Orchestrator
│   │   ├── source_registry.rs       # Source registration management
│   │   ├── session_manager.rs       # Session lifecycle
│   │   └── statistics.rs            # Aggregated statistics
│   │
│   ├── ports/                       # Interface definitions
│   │   ├── mod.rs
│   │   ├── inbound/
│   │   │   ├── mod.rs
│   │   │   ├── ingest_port.rs       # IngestPort trait
│   │   │   └── control_port.rs      # ControlPort trait
│   │   └── outbound/
│   │       ├── mod.rs
│   │       ├── publish_port.rs      # PublishPort trait
│   │       └── event_port.rs        # EventPort trait
│   │
│   └── adapters/                    # Concrete implementations
│       ├── mod.rs
│       ├── inbound/
│       │   ├── mod.rs
│       │   ├── grpc/
│       │   │   ├── mod.rs
│       │   │   └── replay_receiver.rs  # Tonic gRPC server for Replay Simulator
│       │   └── rest/
│       │       ├── mod.rs
│       │       ├── router.rs           # Axum router setup
│       │       └── handlers.rs         # REST endpoint handlers
│       └── outbound/
│           ├── mod.rs
│           └── rabbitmq/
│               ├── mod.rs
│               ├── publisher.rs        # Lapin telemetry publisher
│               └── event_publisher.rs  # Lapin event publisher
│
├── configs/
│   ├── default.yaml
│   ├── development.yaml
│   └── production.yaml
│
├── deployments/
│   ├── Dockerfile
│   └── docker-compose.yml
│
├── docs/                             # This documentation
│
├── scripts/
│   └── test_publish.sh               # Smoke test script
│
└── tests/
    ├── integration/
    │   ├── mod.rs
    │   ├── ingestion_test.rs
    │   ├── rabbitmq_test.rs
    │   └── api_test.rs
    ├── mocks/
    │   ├── mod.rs
    │   ├── mock_publisher.rs
    │   └── mock_source.rs
    └── fixtures/
        └── sample_envelopes.json
```

### Why This Structure (Rust-Specific)

| Decision | Rationale |
|----------|-----------|
| Standard library layout | Follows Cargo standards (`src/` with `main.rs`, module files). |
| Decoupled domain | Domain modules have no dependencies on Axum, Tonic, or Lapin, keeping logic pure and unit-testable. |
| Traits in `ports/` | Explicit interface isolation using Rust traits. Allows mock generation for tests. |

---

## 10. Revision History

| Version | Date       | Description |
|---------|------------|-------------|
| 1.0.0   | 2026-07-03 | Initial draft |

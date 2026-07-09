# MuST Telemetry Gateway — Sequence Diagrams

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-GW-SEQ-004                          |
| **Version**        | 1.0.0-DRAFT                             |
| **Date**           | 2026-07-03                               |
| **Status**         | DRAFT — PENDING REVIEW                   |

---

## 1. Source Registration Flow (DEFERRED)

> [!NOTE]
> In Version 1, dynamic registration is deferred. The gateway uses static configuration profiles (mocked in `IngestionOrchestrator::mock_registration`) to assign mission context, satellite identifiers, and ground station contexts.

```mermaid
sequenceDiagram
    Note over Operator, Gateway: Dynamic source registration is deferred to v2.
```

---

## 2. Telemetry Ingestion and Processing Flow

```mermaid
sequenceDiagram
    autonumber
    participant Source as Replay Simulator (gRPC Client)
    participant Ingress as Inbound Adapter (gRPC Server)
    participant Orch as Ingestion Orchestrator (IngestPort)
    participant Norm as Normalizer
    participant Val as Validator
    participant Enr as Enricher
    participant Router as Router
    participant RMQ as RabbitMQ (telemetry.raw)

    Source->>Ingress: StreamTelemetry(stream TelemetryStreamRequest)
    loop For each packet request in stream
        Ingress->>Orch: on_packet_received(envelope)
        Orch->>Norm: normalize(envelope)
        Orch->>Val: validate(envelope)
        Val-->>Orch: Validation Ok (Result::Ok)
        Orch->>Enr: enrich(envelope, registration, seq)
        Note over Enr: Stamps UUID, receive_timestamp, station context
        Orch->>Router: build_routing_key(envelope)
        Router-->>Orch: routing_key (e.g., cy3.sat101.42.raw)
        Orch->>Enr: set_publish_timestamp(envelope)
        Orch->>RMQ: publish(envelope, routing_key)
        RMQ-->>Orch: Confirmation / OK
        Note over Orch: Increment stats.published
    end
    Source->>Ingress: End of Stream (EOF)
    Ingress->>Orch: on_session_eof(session_id)
    Note over Orch: Print Replay Verification Report
    Ingress-->>Source: TelemetryStreamResponse (Stats)
```

---

## 3. Validation Failure and Rejection Flow

```mermaid
sequenceDiagram
    autonumber
    participant Source as Replay Simulator (gRPC Client)
    participant Ingress as Inbound Adapter (gRPC Server)
    participant Orch as Ingestion Orchestrator
    participant Val as Validator

    Source->>Ingress: StreamTelemetry(stream TelemetryStreamRequest)
    Ingress->>Orch: on_packet_received(envelope)
    Orch->>Val: validate(envelope)
    Val-->>Orch: Err(GatewayError::InvalidPacket)
    Note over Orch: Increment stats.dropped
    Orch-->>Ingress: Err(GatewayError)
    Ingress-->>Source: Stream Error/Rejection
```

---

## 4. Backpressure and Saturated Buffer Flow

When the RabbitMQ broker experiences connection lag or high network load, Lapin block/wait mechanisms propagate backpressure through the application task loop back to the Tonic streaming stream consumer, forcing the gRPC client to pause.

```mermaid
sequenceDiagram
    autonumber
    participant Source as Replay Simulator
    participant Ingress as Inbound Adapter (gRPC)
    participant Orch as Ingestion Orchestrator
    participant RMQ as RabbitMQ (telemetry.raw)

    Note over Ingress,RMQ: RabbitMQ broker degrades or slows down
    Orch->>RMQ: publish(envelope, key)
    Note over RMQ: Lapin blocks/waits on socket write
    RMQ-->>Orch: Delayed Confirm
    Note over Orch: Ingestion loop blocks awaiting RabbitMQ publish completion
    Source->>Ingress: Send next packets
    Note over Ingress: gRPC window buffer saturates
    Ingress-->>Source: HTTP/2 Flow Control (WINDOW_UPDATE paused)
    Note over Source: Source Replay Engine blocks / pauses playback
```

---

## 5. Force-Termination Flow (DEFERRED)

> [!NOTE]
> Operator-initiated dynamic termination is deferred. In Version 1, the session naturally stops when the gRPC client terminates the connection or sends an EOF.

```mermaid
sequenceDiagram
    Note over Operator, Gateway: Force termination is deferred to v2.
```

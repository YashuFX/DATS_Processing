# MuST Telemetry Gateway — Sequence Diagrams

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-GW-SEQ-004                          |
| **Version**        | 1.0.0-DRAFT                             |
| **Date**           | 2026-07-03                               |
| **Status**         | DRAFT — PENDING REVIEW                   |

---

## 1. Source Registration Flow

```mermaid
sequenceDiagram
    autonumber
    actor Operator
    participant REST as Gateway REST API
    participant Registry as Source Registry
    participant Adapter as TelemetrySource Adapter

    Operator->>REST: POST /gateway/register-source (Payload)
    REST->>Registry: Validate config uniqueness & schema
    alt Validation Fails
        Registry-->>REST: Error (Conflict/Invalid)
        REST-->>Operator: HTTP 400/409 (JSON Error)
    else Validation Succeeds
        Registry->>Registry: Persist Source Record
        Registry->>Adapter: Instantiate source adapter (e.g. GrpcSourceAdapter)
        Adapter-->>Registry: Ready
        Registry-->>REST: Source ID
        REST-->>Operator: HTTP 201 Created (Source ID)
    end
```

---

## 2. Telemetry Ingestion and Processing Flow

```mermaid
sequenceDiagram
    autonumber
    participant Source as Telemetry Source (Replay/Receiver)
    participant Ingress as Inbound Adapter (gRPC)
    participant Orch as Ingestion Orchestrator
    participant Val as Validator
    participant Enr as Enricher
    participant RMQ as RabbitMQ (telemetry.raw)
    participant Stats as Statistics Aggregator

    Source->>Ingress: StreamTelemetry(TelemetryStreamRequest)
    Ingress->>Orch: ProcessPacket(envelope)
    Orch->>Val: Validate(envelope)
    Val-->>Orch: ValidationResult (pass = true)
    Orch->>Enr: Enrich(envelope, metadata)
    Enr->>Enr: Set receive_timestamp, gateway sequence, QualityIndicator
    Enr-->>Orch: Enriched TelemetryEnvelope
    Orch->>RMQ: Publish to telemetry.raw (routing: mission.sat.apid.raw)
    RMQ-->>Orch: Publisher Confirm (ACK)
    Orch->>Stats: RecordSuccess()
    Orch-->>Ingress: Stream OK
    Ingress-->>Source: Stream ack (optional metrics update)
```

---

## 3. Validation Failure and Rejection Flow

```mermaid
sequenceDiagram
    autonumber
    participant Source as Telemetry Source
    participant Ingress as Inbound Adapter
    participant Orch as Ingestion Orchestrator
    participant Val as Validator
    participant Evt as EventPort (must.events)
    participant Stats as Statistics Aggregator

    Source->>Ingress: StreamTelemetry(TelemetryStreamRequest)
    Ingress->>Orch: ProcessPacket(envelope)
    Orch->>Val: Validate(envelope)
    Val-->>Orch: ValidationResult (pass = false, reason = "EMPTY_PAYLOAD")
    Orch->>Evt: Emit(PlatformEvent: "gateway.packet.rejected")
    Orch->>Stats: RecordFailure(reason)
    Orch-->>Ingress: Error Response (Rejected packet)
    Ingress-->>Source: Stream Error/Rejection
```

---

## 4. Backpressure and Saturated Buffer Flow

When the RabbitMQ client experiences connection lag or high network load, the internal Go channel buffers begin to fill. This triggers backpressure to preserve stability.

```mermaid
sequenceDiagram
    autonumber
    participant Source as Telemetry Source
    participant Ingress as Inbound Adapter
    participant Orch as Ingestion Orchestrator
    participant RMQ as RabbitMQ (telemetry.raw)
    participant Evt as EventPort (must.events)

    Note over Ingress,RMQ: RabbitMQ connection degrades or rate limit active
    Orch->>RMQ: Publish
    RMQ-->>Orch: Delay / No ACK (channel blocked)
    Note over Orch: Internal Publish Channel saturates
    Source->>Ingress: Send next packets
    Ingress->>Orch: ProcessPacket(envelope)
    Note over Orch: Ingestion Channel exceeds 90% threshold
    Orch->>Evt: Emit(PlatformEvent: "gateway.queue.full")
    Orch-->>Ingress: Error (Buffer Saturated)
    Ingress-->>Source: TCP/gRPC Flow Control (Block/Wait/Pause signal)
    Note over Source: Source Pauses (ADR-003)
```

---

## 5. Force-Termination Flow

```mermaid
sequenceDiagram
    autonumber
    actor Operator
    participant REST as Gateway REST API
    participant Mgr as Session Manager
    participant Adapter as Inbound Adapter
    participant Evt as EventPort (must.events)

    Operator->>REST: POST /gateway/stop-session (session_id)
    REST->>Mgr: RequestStop(session_id)
    Mgr->>Adapter: Close connection / Stop Stream
    Adapter-->>Mgr: Confirmed Closed
    Mgr->>Evt: Emit(PlatformEvent: "gateway.session.finished")
    Mgr-->>REST: OK
    REST-->>Operator: HTTP 200 OK
```

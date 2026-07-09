# CCSDS Decoder Service — Software Requirements Specification

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-DEC-SRS-001                         |
| **Version**        | 1.0.0                                    |
| **Date**           | 2026-07-09                               |
| **Status**         | APPROVED                                 |

---

## 1. Introduction

### 1.1 Purpose
This document specifies the software requirements for the **CCSDS Decoder Service**, a high-performance, asynchronous Rust microservice. The service consumes raw enveloped telemetry frames, parses and validates CCSDS 133.0-B-2 space packet headers, checks packet sequence continuity, and publishes decorated envelopes to the decoded telemetry exchange.

### 1.2 System Context
The CCSDS Decoder operates downstream of the **Telemetry Gateway** and upstream of the **XTCE Engineering Service**.

```
┌──────────────────┐      telemetry.raw      ┌──────────────────┐      telemetry.decoded      ┌──────────────┐
│                  ├────────────────────────>│  CCSDS Decoder   ├────────────────────────────>│ XTCE Service │
│  Telemetry GW    │   (routing: #.raw)      │  Service         │   (routing: #.decoded)     │ (Downstream) │
└──────────────────┘                         └──────────────────┘                             └──────────────┘
```

---

## 2. Functional Requirements

### 2.1 Ingress Ingestion

| ID | Requirement | Priority | Rationale |
|----|-------------|----------|-----------|
| **DEC-010** | The service SHALL establish a durable consumer on the RabbitMQ message broker. | MUST | High-throughput async processing |
| **DEC-011** | The service SHALL consume `TelemetryEnvelope` messages from the queue bound to the `telemetry.raw` exchange. | MUST | Data plane continuity |
| **DEC-012** | The service SHALL support dynamic QoS prefetch configuration to optimize throughput and ordering. | MUST | Backpressure management |

### 2.2 Packet Parsing & Validation

| ID | Requirement | Priority | Rationale |
|----|-------------|----------|-----------|
| **DEC-020** | The service SHALL deserialize incoming payloads into the canonical Protobuf `TelemetryEnvelope`. | MUST | Inter-service compatibility |
| **DEC-021** | The service SHALL parse the CCSDS Primary Header (Version, Type, Secondary Header Flag, APID, Sequence Flags, Sequence Count, Packet Data Length). | MUST | Conform to CCSDS 133.0-B-2 |
| **DEC-022** | The service SHALL validate that the Space Packet version number equals 0. | MUST | Strict protocol checking |
| **DEC-023** | The service SHALL validate that the packet data length matches the expected physical packet size. | MUST | Framing integrity |
| **DEC-024** | The service SHALL support optional CRC-16 check validation when the `CHECK_CRC` flag is enabled. | MUST | Support both CRC and non-CRC missions |

### 2.3 Secondary Header & Timestamping

| ID | Requirement | Priority | Rationale |
|----|-------------|----------|-----------|
| **DEC-030** | If the secondary header flag is set, the service SHALL parse secondary headers to extract time-codes (coarse and fine components). | MUST | Temporal context resolution |
| **DEC-031** | The service SHALL support the CCSDS Unsegmented Time Code (CUC) format. | MUST | Satellites epoch standard |

### 2.4 Sequence Continuity

| ID | Requirement | Priority | Rationale |
|----|-------------|----------|-----------|
| **DEC-040** | The service SHALL maintain sequence continuity tracking independently for each Space Packet APID. | MUST | Detect packet drops and dupes |
| **DEC-041** | The service SHALL flag gaps (non-sequential increment) and duplicates (matching sequence count) in the quality indicator block. | MUST | Real-time transmission diagnostics |

### 2.5 In-place Mutation & Egress

| ID | Requirement | Priority | Rationale |
|----|-------------|----------|-----------|
| **DEC-050** | The service SHALL promote the envelope `ProcessingStage` to `CCSDS_DECODED` in-place. | MUST | Progressive enrichment architecture |
| **DEC-051** | The service SHALL decorate the envelope with the parsed `ccsds_header` and `ccsds_secondary` proto structures. | MUST | Downstream consumption efficiency |
| **DEC-052** | The service SHALL publish the enriched envelope back to the RabbitMQ broker on the `telemetry.decoded` exchange. | MUST | Distribute processed telemetry |
| **DEC-053** | The egress routing key SHALL match the ingress routing key format with the suffix mutated from `.raw` to `.decoded` (e.g. `mission.sat.apid.decoded`). | MUST | Direct-routing compatibility |
| **DEC-054** | The service SHALL output a execution summary of every processed frame to a Console Sink. | MUST | Local debugging and logging |

---

## 3. Non-Functional Requirements

### 3.1 Performance
- **Throughput**: Processing capability of > 100,000 packets per second.
- **Latency**: Sub-millisecond parsing and validation overhead per packet.

### 3.2 Reliability
- **At-least-once processing**: Telemetry envelopes are acknowledged to RabbitMQ only after successful processing and publishing to the downstream exchange.
- **Fail-fast Configuration**: The service terminates immediately on startup if mandatory connection strings are missing.

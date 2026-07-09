# Mission Identification Service — Vision and Requirements Specification

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-MIS-SRS-001                         |
| **Version**        | 1.0.0                                    |
| **Date**           | 2026-07-09                               |
| **Status**         | PROPOSED                                 |

---

## 1. Vision Document

### 1.1 Purpose & Scope
The **Mission Identification Service (MIS)** is a high-performance, asynchronous Rust microservice in the MuST telemetry ingestion pipeline. Its primary role is to act as the pipeline's **contextual router and enricher**. It inspects CCSDS-decoded telemetry packets, maps them to a specific space mission and spacecraft (satellite) instance using a rule-based registry, enriches the packet envelope with authoritative identifiers, and publishes them for downstream calibration and decommutation (XTCE Service).

### 1.2 Problem Statement
In a multi-station, multi-mission ground segment:
- Telemetry packets from different satellites often arrive over shared TCP/UDP links or SDR receiver interfaces.
- The CCSDS APID (Application Process Identifier) is only 11 bits (0 to 2047) and is frequently reused across different spacecraft (e.g., APID 10 might represent thermal data on Satellite A, but battery data on Satellite B).
- Downstream processing services (like XTCE calibration) require a globally unique Mission and Satellite context to load the correct telemetry database.
- The Telemetry Gateway performs ingress routing but may lack the packet-level context (like secondary headers or virtual channel IDs) to distinguish between spacecraft streaming over a combined multiplexed link.

### 1.3 System Boundary & Context
The Mission Identification Service sits between the **CCSDS Decoder Service** and the **XTCE Service**.

```
┌──────────────────┐    telemetry.decoded    ┌──────────────────┐    telemetry.identified    ┌──────────────┐
│  CCSDS Decoder   ├────────────────────────>│  Mission Ident.  ├───────────────────────────>│ XTCE Service │
│  (Decoded Hdr)   │   (routing: #.decoded)  │  Service (MIS)   │   (routing: #.identified)  │ (Calibrate)  │
└──────────────────┘                         └──────────────────┘                            └──────────────┘
```

---

## 2. Functional Requirements (FRS)

### 2.1 Message Consumption
- **FRS-1.1**: MUST consume messages from the `telemetry.decoded` exchange on the RabbitMQ message bus.
- **FRS-1.2**: MUST support wildcard queue bindings to capture all decoded packets (e.g., binding to `#.decoded`).

### 2.2 Rule-Based Identification Engine
- **FRS-2.1**: MUST match incoming packets to a Mission and Satellite based on a configurable rule registry.
- **FRS-2.2**: Rules MUST support matching on:
  - Ingress `source_id` (the physical/logical source of the stream).
  - CCSDS `apid` (Application Process Identifier).
  - CCSDS `vcid` (Virtual Channel Identifier, if present).
- **FRS-2.3**: MUST evaluate rules in order of specificity (e.g., exact `source_id` + `apid` matches take precedence over general `apid`-only rules).

### 2.3 Envelope Enrichment
- **FRS-3.1**: MUST populate the `mission` field of the `TelemetryEnvelope` with the matching `MissionIdentifier` (id, name, code).
- **FRS-3.2**: MUST populate the `satellite` field of the `TelemetryEnvelope` with the matching `SatelliteIdentifier` (id, name, norad_id).
- **FRS-3.3**: MUST update the `stage` of the envelope to `PROCESSING_STAGE_IDENTIFIED`.

### 2.4 Egress Ingest Routing
- **FRS-4.1**: MUST publish the enriched envelope to the downstream exchange (`telemetry.identified`).
- **FRS-4.2**: MUST format the outbound routing key to match the schema: `{mission_code}.{satellite_id}.{apid}.identified`.

---

## 3. Software Requirements (SRS)

### 3.1 Functional Requirements Matrix

| ID | Requirement | Priority | Rationale |
|----|-------------|----------|-----------|
| **MIS-010** | Consumes `TelemetryEnvelope` messages from queue `mission.identify` bound to exchange `telemetry.decoded`. | MUST | Pipeline integration |
| **MIS-020** | Loads a static Mission Registry YAML configuration file on startup. | MUST | Configuration management |
| **MIS-021** | Performs in-memory lookup matching the envelope `source_id` and `apid` against registry rules. | MUST | Low latency lookup |
| **MIS-022** | Detects rule conflicts (multiple matching rules with different outcomes) and log warnings. | MUST | Configuration integrity |
| **MIS-030** | Mutates `TelemetryEnvelope.mission` with resolved `MissionIdentifier` details. | MUST | Downstream metadata |
| **MIS-031** | Mutates `TelemetryEnvelope.satellite` with resolved `SatelliteIdentifier` details. | MUST | Downstream metadata |
| **MIS-032** | Promotes `TelemetryEnvelope.stage` to `PROCESSING_STAGE_IDENTIFIED`. | MUST | Stage tracking |
| **MIS-033** | Appends validation results to `TelemetryEnvelope.quality.warnings` if identification is ambiguous. | MUST | Observability |
| **MIS-040** | Publishes the enriched envelope to `telemetry.identified` exchange. | MUST | Downstream routing |
| **MIS-041** | Translates routing keys replacing the `.decoded` suffix with `.identified`. | MUST | Routing key compliance |
| **MIS-042** | Implements RabbitMQ publisher confirms to guarantee delivery before message ACK. | MUST | At-least-once processing |
| **MIS-050** | Rejects and sends to Dead Letter Queue (DLQ) any packets that cannot be matched to a mission. | MUST | Prevention of queue blocking |

### 3.2 Non-Functional Requirements

- **Performance (Latency)**: In-memory rule matching must execute in under 100 microseconds per packet.
- **Performance (Throughput)**: Capable of processing at least 150,000 packets/sec on single-core deployment.
- **Memory Footprint**: Total memory footprint must remain under 100 MB.
- **Reliability**: Zero packet loss under network partition or RabbitMQ broker reconnection events.

---

## 4. Validation & Identification Rules

The lookup engine evaluates the following criteria to map a packet to a mission/satellite:

```
                  Incoming Telemetry Envelope
                              │
                              ▼
                 Is there an exact match for?
              [source_id + vcid + apid] Rule
                 /                        \
              (Yes)                       (No)
               /                            \
      Apply Identifiers                Is there an exact match for?
              │                        [source_id + apid] Rule
              │                           /                   \
              │                        (Yes)                  (No)
              │                         /                       \
              │                Apply Identifiers           Is there a match for?
              │                         │                   [apid] Rule
              │                         │                     /       \
              │                         │                  (Yes)      (No)
              │                         │                   /           \
              │                         │          Apply Identifiers    FAIL
              ▼                         ▼                 ▼              ▼
       Enrich Envelope            Enrich Envelope   Enrich Envelope   Send to DLQ
```

1. **Rule Conflict Resolution**: If multiple rules match, the engine selects the rule with the highest specificity (defined as matching the highest number of criteria).
2. **Missing Identifiers**: If a packet contains a populated `MissionIdentifier` from the Telemetry Gateway, the MIS validates that the packet is indeed allowed under the registry. If it mismatches, it is flagged as a validation failure.

---

## 5. Error Handling Strategy

| Error Scenario | Detection Mechanism | Resolution / Mitigation |
|----------------|---------------------|------------------------|
| **Unregistered APID** | No match found in registry. | Log warning, increment counter, NACK packet to Dead Letter Queue (DLQ). |
| **Malformed Envelope** | Protobuf deserialization fails. | Log error, NACK packet directly to DLQ. |
| **AMQP Connection Drop** | `lapin` heartbeat or publish fails. | Trigger backpressure, halt consumption, retry connection using exponential backoff. |
| **Publisher Timeout** | No broker confirm within `PUBLISH_TIMEOUT_MS`.| Retry publishing up to `RETRY_MAX_ATTEMPTS` times. If all fail, crash/restart service to trigger container orchestration. |

---

## 6. What the Service MUST and MUST NOT Do

### 6.1 What the Service MUST Do
- Consume protobuf telemetry envelopes asynchronously from RabbitMQ.
- Match packets to a mission database quickly using in-memory indices.
- Stamp the authoritative `MissionIdentifier` and `SatelliteIdentifier` onto the envelope.
- Route identified packets to the next logical pipeline bus (`telemetry.identified`).
- Ensure at-least-once processing via publisher confirms and manual ACKs.

### 6.2 What the Service MUST NOT Do
- It **MUST NOT** parse XTCE parameter databases or perform telemetry decommutation.
- It **MUST NOT** perform physical bit validations or structural CRC-16 checks.
- It **MUST NOT** store packets to cold database storage.
- It **MUST NOT** modify the raw packet binary payload itself.

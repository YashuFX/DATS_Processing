# XTCE Decoder Service — Vision and Requirements Specification

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-XTCE-SRS-001                        |
| **Version**        | 1.0.0                                    |
| **Date**           | 2026-07-10                               |
| **Status**         | PROPOSED                                 |

---

## 1. Introduction

### 1.1 Purpose
This document specifies the software requirements for the **XTCE Decoder Service**, a high-performance, asynchronous Rust microservice in the MuST telemetry acquisition pipeline. The service is responsible for consuming identified telemetry envelopes, dynamically loading XML Telemetry and Command Exchange (XTCE) databases per mission, decommutating raw binary payloads into parameters, applying mathematical calibration curves, and publishing the enriched engineering telemetry envelope to the next processing stage.

### 1.2 System Context
The XTCE Decoder Service is positioned downstream of the **Mission Identification Service** and upstream of the **Validation Service** and other downstream consumer services (e.g., dashboard, database archivers).

```
┌────────────────────────┐      telemetry.identified      ┌────────────────────────┐      telemetry.engineering      ┌────────────────────────┐
│                        ├───────────────────────────────>│      XTCE Decoder      ├────────────────────────────────>│   Validation Service   │
│ Mission Identification │   (routing: #.identified)      │      Service           │   (routing: #.engineering)      │   (Downstream)         │
└────────────────────────┘                                └────────────────────────┘                                 └────────────────────────┘
```

---

## 2. Vision & Scope

### 2.1 Vision
To establish a mission-independent, zero-allocation-friendly, high-throughput decommutation and calibration engine. The engine uses the international ECSS-E-ST-70-31C / CCSDS 660.0-B-2 XTCE standard XML format to declare spacecraft telemetry structures, removing hardcoded decoding parameters and enabling the ingestion of new satellites through metadata updates alone.

### 2.2 Scope
- **In-Scope**:
  - Consuming Protobuf telemetry envelopes from RabbitMQ.
  - Dynamically retrieving XML-based XTCE database schema based on the mission identifier.
  - Parsing XTCE schema elements: SpaceSystems, SequenceContainers (including container inheritance), Parameters, and Calibrators.
  - Thread-safe, in-memory caching of parsed XTCE models.
  - Decommutation of raw packet payloads at bit-level offsets.
  - Mathematical calibration (Polynomial, Spline/Interpolation, and State translation).
  - Appending decoded parameters into the Protobuf envelope.
  - Downstream publication to RabbitMQ with publish confirmations.
- **Out-of-Scope**:
  - Processing raw frames prior to CCSDS validation and frame-sync (handled by Telemetry Gateway/CCSDS Decoder).
  - Resolving mission and satellite IDs based on heuristics (handled by Mission Identification Service).
  - Persisting time-series database records (handled by Archive Service).
  - Out-of-bounds limit alarms and alerts (handled by Validation & Alarm Services).

---

## 3. Functional Requirements (FRS)

### 3.1 Ingress & Dispatch

| ID | Requirement | Priority | Rationale |
|----|-------------|----------|-----------|
| **XTCE-010** | The service SHALL establish a durable consumer on the `telemetry.identified` exchange. | MUST | Asynchronous data plane consumption. |
| **XTCE-011** | The service SHALL bind its queue (`xtce.process`) to the exchange using the wildcard pattern `#.identified`. | MUST | Ingest all identified mission packets. |
| **XTCE-012** | The service SHALL support configurable QoS prefetch limits (default `50`). | MUST | Ensure flow control and backpressure management. |
| **XTCE-013** | The service SHALL reject envelopes that are missing the `mission` identifier field. | MUST | Core lookup dependency. |

### 3.2 Database & Registry Management

| ID | Requirement | Priority | Rationale |
|----|-------------|----------|-----------|
| **XTCE-020** | The service SHALL maintain a thread-safe, in-memory registry of parsed XTCE XML databases (`XtceRegistry`). | MUST | Avoid high XML parsing overhead per telemetry packet. |
| **XTCE-021** | The service SHALL dynamically load the appropriate XTCE database file from disk using the envelope's `mission.mission_code` as the lookup key. | MUST | Support multi-mission, concurrent operations. |
| **XTCE-022** | The service SHALL validate loaded XTCE XML schemas against standard XTCE XSD schemas. | MUST | Prevent malformed definition crashes. |
| **XTCE-023** | The service SHALL support reloading the cached XTCE databases via a dedicated system event or configuration change. | SHOULD | Support hot-swapping databases without service restarts. |

### 3.3 Decommutation Engine (Bit-level Extraction)

| ID | Requirement | Priority | Rationale |
|----|-------------|----------|-----------|
| **XTCE-030** | The service SHALL match the packet's `apid` to its corresponding `SequenceContainer` in the loaded XTCE database. | MUST | Resolve the correct binary layout. |
| **XTCE-031** | The service SHALL extract parameter fields at arbitrary bit offsets and lengths defined in the container. | MUST | Support non-byte-aligned spacecraft telemetry. |
| **XTCE-032** | The service SHALL handle standard primitive parameter types: Signed/Unsigned Integers, Floats, Strings, Booleans, and raw Bytes. | MUST | Standard spacecraft parameter capabilities. |
| **XTCE-033** | The service SHALL support big-endian and little-endian byte ordering as specified in the XTCE definition. | MUST | Support diverse onboard computer architectures. |

### 3.4 Calibration Engine (Engineering Conversion)

| ID | Requirement | Priority | Rationale |
|----|-------------|----------|-----------|
| **XTCE-040** | The service SHALL support **Polynomial Calibrators** mapping raw values to engineering numbers ($y = \sum a_n x^n$). | MUST | Standard analog sensor calibration (e.g. thermistors). |
| **XTCE-041** | The service SHALL support **Spline/Interpolation Calibrators** (look-up tables mapping raw nodes to engineering points). | MUST | Support non-linear sensors. |
| **XTCE-042** | The service SHALL support **State/Enumeration Calibrators** mapping integer values to text states (e.g., `0` -> `OFF`, `1` -> `ON`). | MUST | Support status flag conversions. |
| **XTCE-043** | If a parameter lacks a calibrator, the service SHALL output the raw value as the engineering value. | MUST | Support uncalibrated digital counters/registers. |

### 3.5 Enrichment & Egress

| ID | Requirement | Priority | Rationale |
|----|-------------|----------|-----------|
| **XTCE-050** | The service SHALL preserve the original raw packet bytes in the envelope. | MUST | Support telemetry auditing and reprocessing. |
| **XTCE-051** | The service SHALL populate a list of decoded parameters (name, raw value, calibrated value, and validity flag) in the envelope. | MUST | Downstream consumer consumption. |
| **XTCE-052** | The service SHALL promote the envelope's `stage` to `PROCESSING_STAGE_ENGINEERING`. | MUST | Maintain pipeline stage audit log. |
| **XTCE-053** | The service SHALL publish the mutated envelope to the `telemetry.engineering` exchange. | MUST | Telemetry data distribution. |
| **XTCE-054** | The egress routing key format SHALL be `{mission_code}.{satellite_id}.{apid}.engineering`. | MUST | Downstream topic binding routing. |

---

## 4. Software Requirements (SRS)

### 4.1 Non-Functional Performance
- **Throughput**: A single instance of the XTCE Decoder Service SHALL process $\ge 50,000$ packets/second on standard 2-core cloud deployments.
- **Latency**: End-to-end processing time (deserialization, lookup, decommutation, calibration, serialization, publishing) SHALL be $< 2$ milliseconds at the 99th percentile.
- **Memory Footprint**: The service's resident set size (RSS) SHALL NOT exceed 128 MiB during nominal operations.

### 4.2 Reliability & Operational Safety
- **At-Least-Once Delivery**: The service MUST NOT acknowledge (ACK) a consumed message to RabbitMQ until it has received a positive publish confirmation (ACK) from RabbitMQ for the downstream engineering message.
- **DLQ Fault Isolation**: Malformed packets (e.g., length mismatch, serialization failure) or missing mission definitions MUST NOT block the queue. They MUST be sent to the Dead Letter Queue (DLQ) with error annotations.
- **Concurrency Safety**: The cache of XTCE databases and sequence engines MUST be thread-safe, utilizing non-blocking read paths (e.g., `Arc` and `RwLock`).

---

## 5. System Responsibilities

### 5.1 What it MUST do
- Consume incoming envelopes from `telemetry.identified` asynchronously.
- Load the correct XTCE database XML file dynamically based on the mission code.
- Cache parsed databases in-memory for subsequent fast lookups.
- Perform bit-level parsing (unaligned offsets) of raw space packet payloads.
- Apply mathematical calibrations to convert raw counts to physical values.
- Enrich the original protobuf `TelemetryEnvelope` with the extracted parameters list.
- Keep the raw packet unchanged inside the envelope.
- Route failing or corrupted messages to the Dead Letter Queue (`must.dlx`).

### 5.2 What it MUST NOT do
- Do NOT perform database queries or HTTP requests during telemetry processing (all databases must be stored locally on disk and cached in memory).
- Do NOT modify the `envelope_id`, `sequence_number`, or other identifiers.
- Do NOT discard raw packets (loss of raw source packets is strictly forbidden).
- Do NOT hardcode telemetry formats, calibration constants, or packet layouts in the source code.

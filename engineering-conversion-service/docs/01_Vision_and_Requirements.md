# Engineering Conversion Service — Vision and Requirements Specification

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-ECS-SRS-001                        |
| **Version**        | 1.0.0                                    |
| **Date**           | 2026-07-10                               |
| **Status**         | PROPOSED                                 |

---

## 1. Introduction

### 1.1 Purpose
This document specifies the software requirements for the **Engineering Conversion Service**, a high-performance, asynchronous Rust microservice in the MuST telemetry processing pipeline. The service is responsible for consuming decommutated telemetry envelopes, applying configurable mission-independent mathematical and logical formulas to compute derived engineering parameters, enriching the envelope with these calculated parameters, and publishing the enriched envelope downstream to the Validation & Alarm Services.

### 1.2 System Context
The Engineering Conversion Service sits downstream of the **XTCE Decoder Service** and upstream of the **Validation Service** in the telemetry acquisition pipeline.

```
┌─────────────────┐      telemetry.engineering      ┌────────────────────────┐      telemetry.engineering      ┌────────────────────┐
│                 ├────────────────────────────────>│ Engineering Conversion │────────────────────────────────>│ Validation Service │
│  XTCE Decoder   │   (routing: #.decommutated)     │       Service          │    (routing: #.engineering)    │    (Downstream)    │
└─────────────────┘                                 └────────────────────────┘                                 └────────────────────┘
```

---

## 2. Vision & Scope

### 2.1 Vision
To establish a highly flexible, mission-independent, and performant derived telemetry computation engine. The service enables operators to define new, computed parameters (e.g., total power, temperature gradients, or redundant logic states) using a simple configuration schema (YAML/JSON) without modifying the spacecraft's XTCE database or altering any source code.

### 2.2 Scope
* **In-Scope**:
  * Consuming Protobuf telemetry envelopes from the `telemetry.engineering` exchange.
  * Dynamically loading, validating, and caching mission-specific derived parameter configuration files from disk.
  * Extracting input parameter values from the incoming `TelemetryEnvelope.parameters` list.
  * Safe mathematical and logical evaluation of expressions at runtime (e.g. basic algebra, conditional statements, logical checks).
  * Generating new derived parameters and appending them to the envelope's `parameters` list.
  * Promoting the envelope's `stage` to `PROCESSING_STAGE_ENGINEERING_CONVERTED`.
  * Publishing enriched envelopes back to `telemetry.engineering` with the routing key suffix `.engineering` using RabbitMQ publisher confirmations.
* **Out-of-Scope**:
  * Parsing raw frames or CCSDS headers (handled by Gateway and CCSDS Decoder).
  * Direct extraction of parameters from binary payloads or applying XTCE calibration curves (handled by XTCE Decoder).
  * Limit checking or triggering alarms (handled by Validation Service & Alarm Service).
  * Persistence of data to disk or time-series databases (handled by Archive Service).

---

## 3. Functional Requirements (FRS)

### 3.1 Ingress & Dispatch

| ID | Requirement | Priority | Rationale |
|----|-------------|----------|-----------|
| **ECS-010** | The service SHALL establish a durable consumer on the `telemetry.engineering` exchange. | MUST | Asynchronous data plane consumption. |
| **ECS-011** | The service SHALL bind its queue (`engineering.convert`) to the exchange using the routing pattern `#.decommutated`. | MUST | Ingest decommutated telemetry envelopes published by the XTCE Decoder. |
| **ECS-012** | The service SHALL support configurable QoS prefetch limits (default `50`). | MUST | Ensure flow control and prevent memory exhaustion under heavy load. |
| **ECS-013** | The service SHALL discard or dead-letter envelopes that are missing the `mission` identifier. | MUST | Mission context is required to load derived parameter rules. |

### 3.2 Computation Configuration Registry

| ID | Requirement | Priority | Rationale |
|----|-------------|----------|-----------|
| **ECS-020** | The service SHALL maintain an in-memory, thread-safe cache registry of parsed derived parameter configurations. | MUST | Minimize filesystem access and parsing overhead per telemetry envelope. |
| **ECS-021** | The service SHALL load the derived parameter configuration file (`{mission_code}.yaml`) from a configured directory on disk based on the envelope's `mission.mission_code` field. | MUST | Support multi-mission isolation and runtime configurability. |
| **ECS-022** | The service SHALL validate that derived parameter configuration files are well-formed and contain no cyclic dependencies among calculated parameters. | MUST | Prevent evaluation loops and runtime crashes. |
| **ECS-023** | The service SHALL support dynamic configuration reload via system signal (SIGHUP) or configuration change events. | SHOULD | Support hot-swapping calculation rules without service downtime. |

### 3.3 Engineering Computation Engine

| ID | Requirement | Priority | Rationale |
|----|-------------|----------|-----------|
| **ECS-030** | The engine SHALL extract parameter values from the incoming envelope's `parameters` list to use as formula inputs. | MUST | Derived values depend on decommutated parameters. |
| **ECS-031** | The engine SHALL support standard mathematical operations: Addition (`+`), Subtraction (`-`), Multiplication (`*`), Division (`/`), Power (`^`), Modulo (`%`), and standard functions (e.g., `abs`, `sin`, `cos`, `sqrt`). | MUST | Essential math requirements for physical telemetry modeling (e.g. $P = V \cdot I$). |
| **ECS-032** | The engine SHALL support logical and relational operations: Logical AND (`&&`), OR (`||`), NOT (`!`), and comparisons (`==`, `!=`, `<`, `>`, `<=`, `>=`). | MUST | Support status logic verification and state-based calculations. |
| **ECS-033** | The engine SHALL support conditional expressions (ternary logic or `if-else` blocks). | MUST | Support branching evaluation (e.g., if heater is active, use thermal coefficient A, else B). |
| **ECS-034** | The engine SHALL handle inputs of different data types (Float, Integer, Boolean, String) and perform safe type promotion where appropriate. | MUST | Telemetry contains mixed digital and analog values. |

### 3.4 Enrichment & Egress

| ID | Requirement | Priority | Rationale |
|----|-------------|----------|-----------|
| **ECS-040** | The service SHALL preserve all original metadata, raw packet bytes, and previously decoded parameters in the envelope. | MUST | Avoid data loss and ensure downstream pipeline auditability. |
| **ECS-041** | The service SHALL append computed derived parameters to the `TelemetryEnvelope.parameters` list. | MUST | Maintain a unified parameter stream for downstream validators and archivers. |
| **ECS-042** | The service SHALL promote the envelope's `stage` to `PROCESSING_STAGE_ENGINEERING_CONVERTED`. | MUST | Audit log progression in the telemetry processing pipeline. |
| **ECS-043** | The service SHALL publish the enriched envelope back to the `telemetry.engineering` exchange. | MUST | Distribute enriched data. |
| **ECS-044** | The outbound routing key format SHALL be `{mission_code}.sat{satellite_id}.{apid}.engineering`. | MUST | Allow Validation and other downstream services to consume via standard `#.engineering` bindings. |

---

## 4. Software Requirements (SRS)

### 4.1 Non-Functional Performance
* **Throughput**: A single instance of the service SHALL process $\ge 50,000$ telemetry envelopes per second on a standard 2-core cloud virtual machine.
* **Latency**: The average processing latency per envelope (deserialization, registry lookup, expression evaluation, mutation, serialization, publishing) SHALL be $< 1.5$ milliseconds at the 99th percentile.
* **Resource Constraints**: The resident set size (RSS) of the service's process SHALL NOT exceed 128 MiB during nominal operations.

### 4.2 Reliability & Operational Safety
* **At-Least-Once Delivery**: The service MUST NOT acknowledge (ACK) a consumed message to RabbitMQ until it has received a positive publish confirmation (ACK) from the broker for the downstream enriched message.
* **DLQ Routing**: Malformed envelopes or configurations MUST NOT block the queue. They must be routed to the Dead Letter Queue (`must.dlx`) with descriptive error annotations.
* **Exception Isolation**: Runtime mathematical failures (e.g., divide-by-zero, missing input parameters, nan values) MUST NOT crash the service. The service SHALL flag the derived parameter's validity as `PARAMETER_VALIDITY_INVALID`, log a warning, and proceed with the remaining calculations and envelope publication.

---

## 5. System Responsibilities

### 5.1 What it MUST do:
* Consume incoming envelopes from the `telemetry.engineering` exchange with routing key `*.decommutated` asynchronously.
* Load, compile, and cache derived parameter configuration files (`{mission_code}.yaml`) dynamically.
* Extract values of decommutated input parameters from envelopes and map them to local variables.
* Safely evaluate configured mathematical and logical formulas.
* Generate and append new parameters to `TelemetryEnvelope.parameters`.
* Stamp the envelope's processing stage as `PROCESSING_STAGE_ENGINEERING_CONVERTED`.
* Route corrupted messages or unrecoverable failures to the Dead Letter Queue.

### 5.2 What it MUST NOT do:
* Do NOT parse CCSDS space packets or decode packet headers.
* Do NOT read XTCE XML databases or evaluate XTCE-level calibrations.
* Do NOT identify missions or satellites (it assumes they are already identified and stamped on the envelope).
* Do NOT modify the original raw packet bytes or delete any previously decoded/calibrated parameters from the envelope.
* Do NOT perform database queries, HTTP requests, or block the event loop with synchronous network/disk calls during real-time telemetry processing.

# MuST Replay Simulator Service — Software Requirements Specification

| Field              | Value                                          |
|--------------------|-------------------------------------------------|
| **Document ID**    | MUST-SIM-SRS-001                                |
| **Version**        | 1.0.0-DRAFT                                    |
| **Classification** | INTERNAL — ENGINEERING                          |
| **Date**           | 2026-07-03                                      |
| **Status**         | DRAFT — PENDING REVIEW                          |
| **Standards**      | ECSS-E-ST-40C, NASA-STD-8739.8, CCSDS 133.0-B-2 |

---

## 1. Purpose

This document defines the complete software requirements for the **Replay Simulator Service** (RSS), a subsystem of MuST.

The RSS replaces a live ground station receiver during development, testing, integration, and training. It reads recorded telemetry files and streams packets to downstream services with timing fidelity indistinguishable from a live source.

**Why this service exists:**
- Live antenna hardware is unavailable during software development.
- Deterministic replay enables reproducible integration testing.
- The downstream telemetry pipeline must be exercised against realistic packet streams.
- When the real receiver is integrated, it implements the same `SourceAdapter` interface — zero changes downstream.

---

## 2. Scope

### 2.1 In Scope

- Binary telemetry file ingestion (`.bin`, `.raw`, `.dat`)
- CCSDS packet file ingestion per CCSDS 133.0-B-2
- Timestamp-preserving replay with configurable speed (1x through 32x)
- Full playback control: start, stop, pause, resume, seek, loop
- REST + gRPC dual API
- Structured event publishing
- Source abstraction via pluggable `SourceAdapter` trait
- Prometheus metrics and health probes

### 2.2 Out of Scope (v1)

- Live TCP/UDP receiver (v2 — interface designed now)
- SDR/GNU Radio integration (v3 — adapter pattern supports it)
- Packet decode / decommutation (downstream Telemetry Processing Service)
- Multi-file concurrent replay (architecture supports future extension)

### 2.3 Acronyms

| Term      | Definition                                           |
|-----------|------------------------------------------------------|
| CCSDS     | Consultative Committee for Space Data Systems        |
| RSS       | Replay Simulator Service                             |
| APID      | Application Process Identifier                       |
| FSM       | Finite State Machine                                 |
| PUS       | Packet Utilization Standard                          |

---

## 3. Functional Requirements

### 3.1 File Ingestion

| ID     | Requirement | Priority | Rationale |
|--------|-------------|----------|-----------|
| FR-010 | SHALL accept binary telemetry files (.bin, .raw, .dat) | MUST | Primary recording format from ground station infrastructure |
| FR-011 | SHALL accept CCSDS packet files with valid Space Packet Protocol headers | MUST | CCSDS 133.0-B-2 is the standard for all MuST missions |
| FR-012 | SHALL support files up to 64 GB without loading entire file into memory | MUST | Extended pass recordings exceed 10 GB. Streaming I/O mandatory |
| FR-013 | SHALL validate file headers and magic bytes before accepting | MUST | Prevents undefined behavior from corrupted files |
| FR-014 | SHALL report file metadata (size, exact packet count, duration) after loading. Packet count is exact because the eager timestamp index scans all packets on LOAD (ADR-002) | MUST | Operators need to assess replay scope. Eager index provides exact counts |
| FR-015 | SHALL support future pcap formats via adapter interface. Deferred to v2 (ADR-004) | SHOULD | PCAP is tertiary format; CCSDS and binary are sufficient for v1 |

### 3.2 Playback Control

| ID     | Requirement | Priority | Rationale |
|--------|-------------|----------|-----------|
| FR-020 | SHALL support commands: START, STOP, PAUSE, RESUME, SEEK, LOOP, SET_SPEED, LOAD_FILE, UNLOAD_FILE, GET_STATUS | MUST | Complete operational control surface |
| FR-021 | SHALL enforce deterministic FSM governing valid command sequences | MUST | Invalid transitions must be rejected, not silently ignored |
| FR-022 | SHALL support playback speeds: 1x, 2x, 4x, 8x, 16x, 32x | MUST | Accelerated replay compresses testing cycles |
| FR-023 | SHALL support single-packet stepping | MUST | Packet-by-packet inspection for debugging |
| FR-024 | SHALL support seeking to arbitrary timestamp within loaded file | MUST | Jump to specific events without replaying from start |
| FR-025 | SHALL support loop mode (auto-restart on EOF) | MUST | Continuous replay for soak testing |
| FR-026 | SHALL support play-until-timestamp mode | SHOULD | Bounded replay for targeted tests |
| FR-027 | SHALL maintain monotonically increasing frame counter | MUST | Downstream gap detection |
| FR-028 | SHALL maintain packet counter, progress percentage, elapsed time, remaining time | MUST | Operator situational awareness |

### 3.3 Timing Fidelity

| ID     | Requirement | Priority | Rationale |
|--------|-------------|----------|-----------|
| FR-030 | SHALL preserve inter-packet timing from original recording at 1x | MUST | Downstream must experience realistic arrival patterns |
| FR-031 | Inter-packet delay SHALL scale by inverse of speed multiplier | MUST | At 2x, 100ms gap becomes 50ms. Mathematical correctness non-negotiable |
| FR-032 | SHALL use monotonic clock, never wall-clock time | MUST | NTP jumps would corrupt replay timing |
| FR-033 | Timing Engine SHALL compensate for processing overhead (drift correction) | MUST | Uncorrected drift compounds over multi-hour replays |
| FR-034 | On PAUSE: freeze replay clock. On RESUME: resume from frozen point, no gap or overlap | MUST | Pause/resume must be transparent to consumers |
| FR-035 | On SEEK: reset replay clock to target, synchronize file reader | MUST | Clean state transition, no orphan packets |

### 3.4 Source Abstraction

| ID     | Requirement | Priority | Rationale |
|--------|-------------|----------|-----------|
| FR-040 | SHALL define SourceAdapter trait abstracting packet acquisition | MUST | Hexagonal architecture: domain decoupled from I/O |
| FR-041 | Trait SHALL expose: open, read_next_packet, seek, close, metadata | MUST | Minimum surface for file replay and live receivers |
| FR-042 | Replay engine SHALL interact exclusively through SourceAdapter | MUST | Testable with mocks, extensible to live sources |
| FR-043 | File replay adapter SHALL implement SourceAdapter for binary and CCSDS | MUST | First concrete implementation validates the interface |

### 3.5 Publishing

| ID     | Requirement | Priority | Rationale |
|--------|-------------|----------|-----------|
| FR-050 | SHALL publish each packet to downstream via RabbitMQ (telemetry.raw exchange) as a TelemetryEnvelope (see Shared Contracts) | MUST | RabbitMQ is the platform-wide bus (ADR-001/006). Enables decoupled fan-out to Gateway, CCSDS, Archive |
| FR-051 | Published TelemetryEnvelope SHALL include: envelope_id, sequence_number, source, mission, satellite, original_timestamp, receive_timestamp, raw_packet, apid, stage | MUST | Shared contract fields enable routing and end-to-end correlation |
| FR-052 | SHALL emit platform events (PlaybackStarted/Paused/Resumed/Finished/Error/StatusChanged) to RabbitMQ (must.events exchange) | MUST | RabbitMQ events enable decoupled monitoring and orchestration (ADR-001) |

### 3.6 API

| ID     | Requirement | Priority | Rationale |
|--------|-------------|----------|-----------|
| FR-060 | SHALL expose REST API for playback control and status | MUST | Universal interface for dashboards, scripts, CI |
| FR-061 | SHALL expose gRPC API for control and telemetry streaming | MUST | Efficient binary streaming for high throughput |
| FR-062 | Both APIs SHALL return consistent schemas and error codes | MUST | Dual-API consistency prevents integration bugs |
| FR-063 | REST API SHALL use standard HTTP status codes | MUST | Standard HTTP semantics |

### 3.7 Error Handling

| ID     | Requirement | Priority | Rationale |
|--------|-------------|----------|-----------|
| FR-070 | SHALL transition to ERROR state on unrecoverable errors | MUST | Fail-visible, not fail-silent |
| FR-071 | SHALL handle: missing file, corrupted file, invalid CCSDS, timestamp corruption, EOF, OOM, invalid command | MUST | Comprehensive error taxonomy |
| FR-072 | Recoverable errors (single corrupted packet) SHALL be logged and skipped | SHOULD | Maximize data recovery from partial corruption |
| FR-073 | Every error SHALL produce structured log with code, context, source, recovery action | MUST | Post-incident analysis needs rich diagnostics |

---

## 4. Non-Functional Requirements

### 4.1 Performance

| ID      | Requirement | Target | Rationale |
|---------|-------------|--------|-----------|
| NFR-010 | Inter-packet timing jitter at 1x | < 1 ms | Indistinguishable from live reception |
| NFR-011 | Max sustained throughput at 32x | > 100K pkt/s | High-rate missions at maximum speed |
| NFR-012 | LOAD to READY latency (10 GB file) | < 2 s | Operator responsiveness |
| NFR-013 | Command response latency | < 50 ms | Control must feel instantaneous |

### 4.2 Resources

| ID      | Requirement | Target | Rationale |
|---------|-------------|--------|-----------|
| NFR-020 | Memory RSS for 64 GB files | < 512 MB | Streaming architecture, no full buffering |
| NFR-021 | I/O buffer (configurable) | 8 MB default | Tunable for storage characteristics |
| NFR-022 | CPU at 1x | < 5% single core | RSS is lightweight; heavy work is downstream |

### 4.3 Reliability

| ID      | Requirement | Target |
|---------|-------------|--------|
| NFR-030 | Auto-recovery from recoverable errors | Automatic |
| NFR-031 | No state corruption on restart | Stateless design |
| NFR-032 | MTBF | > 1000 hours |

### 4.4 Observability

| ID      | Requirement |
|---------|-------------|
| NFR-040 | Prometheus metrics at /metrics |
| NFR-041 | Structured JSON logs via tracing crate |
| NFR-042 | Health endpoints: /health/live, /health/ready, /health/startup |

---

## 5. Interfaces

### Upstream

| Interface        | Protocol   | Direction | Description                     |
|------------------|------------|-----------|---------------------------------|
| Telemetry Files  | Filesystem | Input     | Binary and CCSDS recordings     |
| Operator Console | REST/gRPC  | Input     | Playback control commands       |

### Downstream

| Interface         | Protocol        | Direction | Description                  |
|-------------------|-----------------|-----------|------------------------------|
| RabbitMQ Bus      | AMQP            | Output    | Telemetry envelopes on `telemetry.raw` exchange |
| RabbitMQ Bus      | AMQP            | Output    | Platform events on `must.events` exchange |
| Monitoring Stack  | HTTP/Prometheus  | Output    | Metrics scraping             |

---

## 6. Output Data Envelope (TelemetryEnvelope)

| Field             | Type   | Description                                       |
|-------------------|--------|---------------------------------------------------|
| sequence_number   | uint64 | Monotonic counter for this replay session          |
| original_ts       | uint64 | Timestamp from recording (nanoseconds, epoch)      |
| replay_ts         | uint64 | Timestamp when packet was replayed (nanoseconds)   |
| source_id         | string | Identifier of the source adapter instance          |
| file_offset       | uint64 | Byte offset in source file                         |
| payload           | bytes  | Raw packet data                                    |
| payload_size      | uint32 | Size of payload in bytes                           |

---

## 7. Constraints

| ID      | Constraint | Rationale |
|---------|------------|-----------|
| CON-001 | Rust (edition 2021+) | Platform standardization, memory safety without GC |
| CON-002 | Tokio async runtime | Mature async ecosystem |
| CON-003 | Hexagonal Architecture | Testability, modularity, interface-driven design |
| CON-004 | Protobuf-first API definition | Specification-driven development |
| CON-005 | YAML config with env var overrides | Container deployment standard |

---

## 8. Revision History

| Version | Date       | Description    |
|---------|------------|----------------|
| 1.0.0   | 2026-07-03 | Initial draft  |

# MuST — Architectural Decision Log

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-ADL-001                             |
| **Version**        | 1.0.0                                   |
| **Date**           | 2026-07-03                               |
| **Status**         | APPROVED                                 |

---

## ADR-001: Event Delivery Mechanism

| Field | Value |
|-------|-------|
| **Status** | APPROVED |
| **Date** | 2026-07-03 |
| **Context** | The Replay Simulator Service needs to publish events (state changes, errors, telemetry packets) to downstream services. Options considered: Tokio broadcast channel (in-process only), Redis Pub/Sub, NATS, RabbitMQ. |
| **Decision** | **RabbitMQ** |
| **Rationale** | The entire MuST platform uses RabbitMQ as its message bus. Every service in the pipeline (Replay → Gateway → CCSDS → XTCE → Storage) publishes and consumes via RabbitMQ. Introducing a second messaging system creates operational complexity, monitoring fragmentation, and deployment overhead. One bus, one set of skills, one monitoring stack. |
| **Consequences** | All services depend on RabbitMQ availability. RabbitMQ becomes a critical shared dependency. Must be deployed in clustered mode for production reliability. |

---

## ADR-002: Seek Implementation Strategy

| Field | Value |
|-------|-------|
| **Status** | APPROVED |
| **Date** | 2026-07-03 |
| **Context** | SEEK requires finding the packet at a specific timestamp in a multi-GB file. Options: lazy index (build on first seek, fast load) vs. eager index (build on LOAD, instant seek). |
| **Decision** | **Eager Index — build timestamp index on LOAD** |
| **Rationale** | LOAD happens once. SEEK happens many times. An operator replaying a 4 GB file will seek repeatedly to specific events (anomalies, contact windows, etc.). A lazy scan of millions of packets on each seek is unacceptable. The one-time cost at LOAD is amortized across all subsequent seeks. The index is a compact array of (timestamp, file_offset) pairs — approximately 16 bytes per packet. For 500K packets, that's 8 MB of index memory — negligible. |
| **Consequences** | LOAD time increases (must scan entire file to build index). NFR-012 (< 2s for 10 GB) may need revision. Index consumes memory proportional to packet count. Accepted tradeoff: slower LOAD for instant SEEK. |
| **Impact on SRS** | FR-014 updated: metadata now includes exact packet count (not estimate). NFR-012 relaxed for very large files. |

---

## ADR-003: Backpressure Strategy

| Field | Value |
|-------|-------|
| **Status** | APPROVED |
| **Date** | 2026-07-03 |
| **Context** | When the downstream consumer (Telemetry Gateway / RabbitMQ) is slow, the Replay Simulator must decide: buffer (risk OOM), drop (data loss), or pause (affects timing). |
| **Decision** | **Pause** |
| **Rationale** | Telemetry must NEVER be silently dropped. In aerospace systems, every packet matters — a dropped housekeeping packet could mask a thermal anomaly. Buffering is dangerous because a sustained rate mismatch leads to OOM. Pausing is the only option that preserves data integrity: the Replay Scheduler pauses when the publish channel is full, and resumes when the consumer catches up. The Timing Engine already supports pause/resume transparently (ADR-002 validated this). |
| **Consequences** | Replay timing is affected during backpressure. At high speed (32×), backpressure pauses may be frequent. This is acceptable because: (a) the downstream is the bottleneck, not the replay, and (b) no data is lost. The operator sees the RUNNING state with a "backpressure" flag in status. |
| **Implementation** | PublishPort uses a bounded channel. When the channel is full, the Scheduler awaits (Tokio yield). When space appears, it resumes. The Timing Engine offsets session_start by the backpressure duration, identical to PAUSE handling. |

---

## ADR-004: PCAP Support Scope

| Field | Value |
|-------|-------|
| **Status** | APPROVED |
| **Date** | 2026-07-03 |
| **Context** | Should PCAP (packet capture) file support be included in v1? |
| **Decision** | **No. Deferred to v2.** |
| **Rationale** | The primary input format is CCSDS packet files. Binary raw recordings are the secondary format. PCAP is a tertiary format used only when telemetry is captured from network taps. v1 must focus on the critical path. The SourceAdapter interface (FR-040) already accommodates future PCAP support without interface changes. |
| **Consequences** | PCAP users must convert to binary or CCSDS before replay in v1. Conversion tooling may be needed. |

---

## ADR-005: Multi-File Replay

| Field | Value |
|-------|-------|
| **Status** | APPROVED |
| **Date** | 2026-07-03 |
| **Context** | Should the Replay Simulator support concurrent replay of multiple files? |
| **Decision** | **No. Single file, single replay session in v1.** |
| **Rationale** | Simplicity. Multi-file replay introduces complex timing synchronization across files (which file's clock is authoritative?), complex state management (what does PAUSE mean across multiple files?), and complex error handling (one file errors, others continue?). These are solvable problems, but they are not v1 problems. The architecture supports future extension — multiple RSS instances can be deployed for independent replays. |
| **Consequences** | For multi-satellite pass replay, deploy one RSS instance per file. Orchestration is external (CI script, operator). |

---

## ADR-006: Message Bus Technology

| Field | Value |
|-------|-------|
| **Status** | APPROVED |
| **Date** | 2026-07-03 |
| **Context** | MuST needs a platform-wide message bus for inter-service telemetry flow. |
| **Decision** | **RabbitMQ** |
| **Rationale** | RabbitMQ provides: durable queues (no data loss on consumer restart), flexible routing (topic exchanges for telemetry routing by mission/satellite/APID), mature ecosystem (management UI, monitoring, clustering), and the team already has operational experience. Kafka was considered but rejected: telemetry is not a log — it's a pipeline. We need routing, not replay. NATS was considered but rejected: less mature clustering, less operational tooling. |
| **Consequences** | All services must include a RabbitMQ client library. AMQP protocol overhead (~8 bytes per message). RabbitMQ must be deployed and monitored. |

---

## Revision History

| Version | Date       | Description |
|---------|------------|-------------|
| 1.0.0   | 2026-07-03 | Initial 6 decisions recorded |

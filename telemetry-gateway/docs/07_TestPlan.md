# MuST Telemetry Gateway — Test Plan

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-GW-TEST-007                         |
| **Version**        | 1.0.0-DRAFT                             |
| **Date**           | 2026-07-03                               |
| **Status**         | DRAFT — PENDING REVIEW                   |

---

## 1. Testing Strategy

The Telemetry Gateway requires multi-layered validation to ensure near-zero packet loss, low latency, and deterministic routing. The service is tested at three distinct tiers:

```
┌────────────────────────────────────────────────────────┐
│                   Test Tier Pyramids                   │
│                                                        │
│   ┌────────────────────────────────────────────────┐   │
│   │               Integration Tests                │   │
│   │ - Live RabbitMQ, DB boundary                   │   │
│   │ - gRPC Streaming End-to-End                    │   │
│   └──────────────────────┬─────────────────────────┘   │
│                          │                             │
│             ┌────────────▼──────────────┐              │
│             │        Unit Tests         │              │
│             │ - Domain logic isolation  │              │
│             │ - Custom Mocks / Fakes    │              │
│             └────────────┬──────────────┘              │
│                          │                             │
│            ┌─────────────▼───────────────┐             │
│            │      Performance Tests      │             │
│            │ - Ingress stress limits     │             │
│            │ - Memory Leak Analysis      │             │
│            └─────────────────────────────┘             │
└────────────────────────────────────────────────────────┘
```

---

## 2. Unit Test Matrix

Unit tests are written using the standard Rust testing framework (`#[test]`). No external systems (RabbitMQ, network endpoints) are accessed. Mocks (if any) are written manually or using the `mockall` crate.

| Test ID | Target Component | Input Conditions | Expected Outcome |
|---------|------------------|------------------|------------------|
| UT-101 | `Validator` | Empty payload in `raw_packet` | Reject with `EmptyPayload` error. |
| UT-102 | `Validator` | Correct envelope structure | Pass validation (Result::Ok). |
| UT-103 | `Validator` | Sequence gap | Set sequence_continuous flag = false in QualityIndicator. |
| UT-104 | `Validator` | Duplicate sequence number | Set sequence_continuous flag = false and add warning. |
| UT-105 | `Enricher` | Valid raw envelope | Stamped receive_timestamp = Gateway system clock. |
| UT-106 | `Enricher` | Config present in registry | Mission, Satellite, and GroundStation attributes applied correctly. |
| UT-107 | `IngestionOrchestrator`| Source disconnected | Print report, clear session state. |

---

## 3. Integration Test Matrix

Integration tests require Docker containers running RabbitMQ and mock gRPC clients (using Tonic) to simulate raw telemetry senders.

| Test ID | Target Flow | Setup | Verification Steps |
|---------|-------------|-------|--------------------|
| IT-201 | gRPC Telemetry Ingest | Spin up gateway, start mock client | Verify mock messages arrive in RabbitMQ `telemetry.raw` queue. |
| IT-202 | Static Configuration | Invalid configuration profiles | Verify gateway fails to start / loads static configurations correctly. |
| IT-203 | RabbitMQ Recovery | Disconnect RMQ broker during ingestion | Verify gateway handles disconnection gracefully and reconnects. |
| IT-204 | Backpressure | Block RMQ consumer, flood ingress | Verify gRPC stream throttles / halts client stream (TCP/HTTP2 flow control). |

---

## 4. Performance Benchmarks

Performance tests are executed using Rust's `criterion` benchmark suite or custom load generator scripts.

| benchmark | tool | purpose |
|-----------|------|---------|
| `cargo bench` | `criterion` | Micro-benchmarking processing stages (validation, enrichment, routing) |
| Heap profiling | `heaptrack` / `valgrind` | Detect allocations and potential leaks in long-running async tasks |

### 4.1 Stress Test Metrics
- **Target Load**: 200,000 packets per second (double normal peak load).
- **Duration**: 2 hours continuous run.
- **Criteria**:
  - Max RSS memory usage < 500 MB (Rust is highly memory-efficient).
  - CPU usage remains steady (no runaway tokio tasks).
  - Processing latency P95 < 1 ms.
  - Zero dropped messages on network boundaries.

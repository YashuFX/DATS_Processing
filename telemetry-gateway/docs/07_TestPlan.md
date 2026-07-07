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

Unit tests are written in native Go testing framework. No external systems (RabbitMQ, network endpoints) are accessed. Mocks are generated using `mockgen`.

| Test ID | Target Component | Input Conditions | Expected Outcome |
|---------|------------------|------------------|------------------|
| UT-101 | `Validator` | Empty payload in `raw_packet` | Reject with `EMPTY_PAYLOAD` reason. |
| UT-102 | `Validator` | Correct envelope structure | Pass validation. |
| UT-103 | `Validator` | Sequence gap (e.g., last=10, cur=12) | Set sequence_continuous flag = false in QualityIndicator. |
| UT-104 | `Validator` | Duplicate sequence number | Set sequence_continuous flag = false and add warning. |
| UT-105 | `Enricher` | Valid raw envelope | Stamped receive_timestamp = gateway monotonic clock. |
| UT-106 | `Enricher` | Config present in registry | Mission, Satellite, and GroundStation attributes applied correctly. |
| UT-107 | `SessionManager`| Force-terminate request | Session transitions to `TERMINATED` immediately. |

---

## 3. Integration Test Matrix

Integration tests require Docker containers running RabbitMQ and mock gRPC clients to simulate raw telemetry senders.

| Test ID | Target Flow | Setup | Verification Steps |
|---------|-------------|-------|--------------------|
| IT-201 | gRPC Telemetry Ingest | Spin up gateway, start mock client | Verify mock messages arrive in RabbitMQ `telemetry.raw` queue. |
| IT-202 | Source Registration | Dynamic register API call | Verify client can establish gRPC stream *only after* registration. |
| IT-203 | RabbitMQ Recovery | Disconnect RMQ broker during ingestion | Verify gateway buffers messages, then publishes *without loss* after RMQ recovery. |
| IT-204 | Backpressure | Block RMQ consumer, flood ingress | Verify gateway buffers fill, then gRPC stream throttles / halts client stream. |

---

## 4. Performance Benchmarks

Performance tests are executed using specialized tooling (e.g., custom Go load generator and `k6` for APIs).

### 4.1 Stress Test Metrics
- **Target Load**: 200,000 packets per second (double normal peak load).
- **Duration**: 2 hours continuous run.
- **Criteria**:
  - Max heap memory usage < 1.8 GB.
  - CPU usage remains steady (no runaway goroutines).
  - Processing latency P95 < 2 ms.
  - Zero dropped messages on network boundaries.

### 4.2 Leak Detection
- Run `go test -run=None -bench=BenchmarkIngress -memprofile=mem.out` to capture heap allocations.
- Validate profiles in `pprof` to verify zero memory leaks over long-duration stream ingestion.

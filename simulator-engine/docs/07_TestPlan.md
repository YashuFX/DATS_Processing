# MuST Replay Simulator Service — Test Plan

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-SIM-TEST-007                        |
| **Version**        | 1.0.0-DRAFT                             |
| **Date**           | 2026-07-03                               |
| **Status**         | DRAFT — PENDING REVIEW                   |

---

## 1. Testing Philosophy

Testing follows the **Test Pyramid** adapted for systems software:

```
         ╱╲
        ╱  ╲        Manual / Exploratory
       ╱────╲       (Rare, operator workflows)
      ╱      ╲
     ╱  E2E   ╲     End-to-End (Docker, real files)
    ╱──────────╲
   ╱            ╲
  ╱ Integration  ╲   Component integration (real Tokio, mock adapters)
 ╱────────────────╲
╱                  ╲
╱    Unit Tests     ╲  Pure logic (FSM, Timing Engine, validators)
╱────────────────────╲
```

**Why this distribution:**
- **Unit tests** (70%) — fast, deterministic, catch logic errors. The FSM and Timing Engine are pure functions: given state + command → new state.
- **Integration tests** (20%) — verify Tokio task interactions, channel communication, and adapter contracts.
- **E2E tests** (10%) — verify the complete system in Docker with real telemetry files.

---

## 2. Test Categories and Identifiers

All test cases use the format `TC-{CATEGORY}-{NUMBER}`.

| Category | Code | Description |
|----------|------|-------------|
| Ingestion | ING | File loading, validation, metadata extraction |
| Control | CTL | Command handling and state transitions |
| Timing | TIM | Timing engine, delay computation, drift correction |
| Publishing | PUB | Packet publishing, envelope construction |
| API-REST | RAP | REST endpoint behavior |
| API-gRPC | GAP | gRPC service behavior |
| Error | ERR | Error handling and recovery |
| Performance | PRF | Throughput, latency, resource consumption |
| Abstraction | ABS | Source adapter interface compliance |
| State | STM | State machine exhaustive transitions |

---

## 3. Unit Test Cases

### 3.1 State Machine Tests (TC-STM)

| ID | Test | Input | Expected | Traces To |
|----|------|-------|----------|-----------|
| TC-STM-001 | Valid transition: IDLE → READY | State=IDLE, Cmd=LOAD | READY | FR-021 |
| TC-STM-002 | Valid transition: READY → RUNNING | State=READY, Cmd=START | RUNNING | FR-021 |
| TC-STM-003 | Valid transition: RUNNING → PAUSED | State=RUNNING, Cmd=PAUSE | PAUSED | FR-021 |
| TC-STM-004 | Valid transition: PAUSED → RUNNING | State=PAUSED, Cmd=RESUME | RUNNING | FR-021 |
| TC-STM-005 | Valid transition: RUNNING → STOPPED | State=RUNNING, Cmd=STOP | STOPPED | FR-021 |
| TC-STM-006 | Valid transition: RUNNING → COMPLETED | State=RUNNING, Event=EOF | COMPLETED | FR-021 |
| TC-STM-007 | Invalid: START from IDLE | State=IDLE, Cmd=START | Rejected | FR-021 |
| TC-STM-008 | Invalid: PAUSE from IDLE | State=IDLE, Cmd=PAUSE | Rejected | FR-021 |
| TC-STM-009 | Invalid: RESUME from RUNNING | State=RUNNING, Cmd=RESUME | Rejected | FR-021 |
| TC-STM-010 | Recovery: ERROR → READY via LOAD | State=ERROR, Cmd=LOAD | READY | FR-021 |
| TC-STM-011 | Recovery: ERROR → IDLE via UNLOAD | State=ERROR, Cmd=UNLOAD | IDLE | FR-021 |
| TC-STM-012 | Exhaustive invalid transitions | All invalid state-command pairs | All rejected | FR-021 |
| TC-STM-013 | GET_STATUS from every state | All 7 states | Status returned, no transition | FR-020 |

### 3.2 Timing Engine Tests (TC-TIM)

| ID | Test | Input | Expected | Traces To |
|----|------|-------|----------|-----------|
| TC-TIM-001 | 1x speed preserves timing | delta=100ms, speed=1.0 | delay=100ms | FR-030 |
| TC-TIM-002 | 2x speed halves delay | delta=100ms, speed=2.0 | delay=50ms | FR-031 |
| TC-TIM-003 | 4x speed quarters delay | delta=100ms, speed=4.0 | delay=25ms | FR-031 |
| TC-TIM-004 | 32x speed | delta=100ms, speed=32.0 | delay=3.125ms | FR-031 |
| TC-TIM-005 | Zero delta (simultaneous packets) | delta=0, speed=1.0 | delay=0 | FR-030 |
| TC-TIM-006 | Drift correction positive | drift=+5ms | Next delay reduced by 5ms | FR-033 |
| TC-TIM-007 | Drift correction negative clamp | correction > delay | delay=0 (catch-up) | FR-033 |
| TC-TIM-008 | Pause freezes clock | freeze() called | No time elapses | FR-034 |
| TC-TIM-009 | Resume compensates pause | pause 5s, resume | session_start offset by 5s | FR-034 |
| TC-TIM-010 | Seek resets all accumulators | seek(T) | clock=T, drift=0 | FR-035 |
| TC-TIM-011 | Speed change mid-stream | speed 1x→4x during replay | Next delay uses new speed | FR-022 |

### 3.3 Ingestion Tests (TC-ING)

| ID | Test | Input | Expected | Traces To |
|----|------|-------|----------|-----------|
| TC-ING-001 | Load valid binary file | Valid .bin file | READY + metadata | FR-010 |
| TC-ING-002 | Load valid CCSDS file | Valid CCSDS file | READY + metadata | FR-011 |
| TC-ING-003 | Reject missing file | Non-existent path | FILE_NOT_FOUND error | FR-070 |
| TC-ING-004 | Reject corrupted header | Truncated file | INVALID_FILE error | FR-013 |
| TC-ING-005 | Reject path traversal | "../../../etc/passwd" | PATH_TRAVERSAL error | NFR-050 |
| TC-ING-006 | Metadata accuracy | Known test file | Correct size, packet count, duration | FR-014 |
| TC-ING-007 | Large file (1 GB) memory | 1 GB test file | RSS < 512 MB during load | NFR-020 |

### 3.4 Command Validation Tests (TC-CTL)

| ID | Test | Input | Expected | Traces To |
|----|------|-------|----------|-----------|
| TC-CTL-001 | Valid speed values accepted | Each of 1,2,4,8,16,32 | Accepted | FR-022 |
| TC-CTL-002 | Invalid speed rejected | speed=3.0 | INVALID_SPEED | FR-022 |
| TC-CTL-003 | Step mode (speed=0) | speed=0.0 | Single packet, then wait | FR-023 |
| TC-CTL-004 | Seek within range | Valid timestamp | Seek succeeds | FR-024 |
| TC-CTL-005 | Seek out of range | Timestamp beyond EOF | TIMESTAMP_OUT_OF_RANGE | FR-024 |
| TC-CTL-006 | Loop enable | enabled=true | Loop config stored | FR-025 |
| TC-CTL-007 | Loop with max iterations | max=3 | Stops after 3 loops | FR-025 |

---

## 4. Integration Test Cases

### 4.1 Playback Integration (TC-PUB)

| ID | Test | Setup | Verification | Traces To |
|----|------|-------|-------------|-----------|
| TC-PUB-001 | Full file replay | Load small test file, start at 1x | All packets published in order, correct timing | FR-050, FR-030 |
| TC-PUB-002 | Envelope completeness | Replay single packet | Envelope has all fields populated | FR-051 |
| TC-PUB-003 | Sequence monotonicity | Replay 1000 packets | sequence_number strictly increasing | FR-027 |
| TC-PUB-004 | Pause-resume continuity | Pause after 50 packets, resume | No gap, no duplicate, timing correct | FR-034 |
| TC-PUB-005 | Seek accuracy | Seek to known timestamp | Next published packet matches target | FR-024 |
| TC-PUB-006 | Loop restart | Enable loop, replay to EOF | Restarts from beginning, counters reset | FR-025 |
| TC-PUB-007 | Speed change during replay | Change 1x→4x mid-replay | Subsequent delays are quartered | FR-022 |

### 4.2 REST API Integration (TC-RAP)

| ID | Test | Method | Expected |
|----|------|--------|----------|
| TC-RAP-001 | Load file | POST /load | 200 with metadata |
| TC-RAP-002 | Load missing file | POST /load | 404 |
| TC-RAP-003 | Start from IDLE | POST /start | 409 INVALID_STATE |
| TC-RAP-004 | Start from READY | POST /start | 200 |
| TC-RAP-005 | Pause while RUNNING | POST /pause | 200 |
| TC-RAP-006 | Get status | GET /status | 200 with full status |
| TC-RAP-007 | Get statistics | GET /statistics | 200 with timing stats |
| TC-RAP-008 | Health live | GET /health/live | 200 |
| TC-RAP-009 | Health ready | GET /health/ready | 200 |

### 4.3 gRPC Integration (TC-GAP)

| ID | Test | RPC | Expected |
|----|------|-----|----------|
| TC-GAP-001 | Stream telemetry | StreamTelemetry | Receives packets as replayed |
| TC-GAP-002 | Stream events | StreamEvents | Receives state change events |
| TC-GAP-003 | Load via gRPC | LoadFile | SUCCESS with metadata |
| TC-GAP-004 | Invalid state via gRPC | Start from IDLE | FAILED_PRECONDITION |

---

## 5. Error Handling Tests (TC-ERR)

| ID | Test | Trigger | Expected |
|----|------|---------|----------|
| TC-ERR-001 | Missing file | LOAD non-existent path | ERROR state + event |
| TC-ERR-002 | Corrupted CCSDS packet | File with bad header mid-stream | Packet skipped, replay continues |
| TC-ERR-003 | Non-monotonic timestamp | Out-of-order timestamp in file | Warning logged, previous_ts + min_delta used |
| TC-ERR-004 | EOF handling | Normal end of file | COMPLETED state + event |
| TC-ERR-005 | Invalid command from state | PAUSE from IDLE | 409 error, state unchanged |
| TC-ERR-006 | Publisher disconnection | Kill downstream during replay | Retry, then ERROR state |
| TC-ERR-007 | Rapid command sequence | START then immediate STOP | Clean transition, no race |

---

## 6. Performance Tests (TC-PRF)

| ID | Test | Method | Pass Criteria | Traces To |
|----|------|--------|--------------|-----------|
| TC-PRF-001 | Timing jitter at 1x | Replay 10,000 packets, measure jitter | P99 jitter < 1ms | NFR-010 |
| TC-PRF-002 | Throughput at 32x | Replay high-rate file at 32x | > 100K pkt/s sustained | NFR-011 |
| TC-PRF-003 | Load latency | Load 10 GB file, measure time | < 2 seconds | NFR-012 |
| TC-PRF-004 | Command latency | Measure time from API call to state change | < 50ms | NFR-013 |
| TC-PRF-005 | Memory under large file | Load 10 GB file, replay | RSS < 512 MB | NFR-020 |
| TC-PRF-006 | CPU at 1x | Replay at 1x for 60 seconds | < 5% single core | NFR-022 |
| TC-PRF-007 | Long-duration stability | Continuous replay for 24 hours | No memory growth, no drift > 10ms | NFR-032 |

---

## 7. Source Adapter Tests (TC-ABS)

| ID | Test | Adapter | Verification |
|----|------|---------|-------------|
| TC-ABS-001 | Mock adapter compliance | MockSourceAdapter | All trait methods callable, correct return types |
| TC-ABS-002 | Binary reader packet framing | BinaryReaderAdapter | Packets correctly framed from raw bytes |
| TC-ABS-003 | CCSDS reader header parsing | CcsdsReaderAdapter | APID, sequence count, length correctly extracted |
| TC-ABS-004 | Seek implementation | FileReaderAdapter | Seek positions reader at correct packet |
| TC-ABS-005 | EOF signaling | FileReaderAdapter | read_next_packet returns None at EOF |

---

## 8. Test Data Strategy

### 8.1 Synthetic Test Files

A Python script (`scripts/generate_test_data.py`) generates deterministic test files:

| File | Size | Packets | Description |
|------|------|---------|-------------|
| `small_ccsds.bin` | 64 KB | 100 | Basic CCSDS validation |
| `medium_binary.bin` | 10 MB | 5,000 | Integration testing |
| `large_binary.bin` | 1 GB | 500,000 | Performance testing |
| `corrupted_mid.bin` | 1 MB | 500 | Corrupted packet at offset 500KB |
| `non_monotonic_ts.bin` | 1 MB | 500 | Out-of-order timestamp at packet 250 |
| `zero_gap.bin` | 1 MB | 500 | All timestamps identical (burst) |
| `large_gap.bin` | 1 MB | 100 | 30-second gaps between packets |

**Why synthetic:** Real telemetry files are often classified or restricted. Synthetic files with known properties enable deterministic testing.

### 8.2 Test File Invariants

Every synthetic test file includes:
- Known total packet count (for progress verification)
- Known first and last timestamps (for seek/duration verification)
- Known total byte size
- Deterministic content (seeded PRNG for payloads)

---

## 9. CI Pipeline Integration

```
┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
│  Build   │───>│  Lint    │───>│  Unit    │───>│  Integ   │───>│  E2E     │
│          │    │  clippy  │    │  Tests   │    │  Tests   │    │  Tests   │
│          │    │  fmt     │    │          │    │          │    │ (Docker) │
└──────────┘    └──────────┘    └──────────┘    └──────────┘    └──────────┘
                                     │               │               │
                                     ▼               ▼               ▼
                                 Coverage        Coverage        Report
                                  Report          Report
```

| Stage | Tool | Pass Criteria |
|-------|------|--------------|
| Build | `cargo build --release` | Zero errors |
| Lint | `cargo clippy -- -D warnings` | Zero warnings |
| Format | `cargo fmt -- --check` | Zero formatting diffs |
| Unit | `cargo test --lib` | 100% pass |
| Integration | `cargo test --test '*'` | 100% pass |
| Coverage | `cargo llvm-cov` | >= 80% line coverage |
| E2E | `docker compose -f docker-compose.test.yml up` | All scenarios pass |
| Audit | `cargo audit` | Zero known vulnerabilities |

---

## 10. Test Coverage Targets

| Component | Target Coverage | Rationale |
|-----------|----------------|-----------|
| State Machine | 100% | All states and transitions are enumerable |
| Timing Engine | 95% | Core timing math must be exhaustively tested |
| Command Handler | 90% | All validation paths |
| Source Adapters | 85% | File I/O has hard-to-test edge cases |
| REST API | 80% | HTTP layer has framework-handled code |
| gRPC API | 80% | Same rationale as REST |
| Overall | >= 80% | Industry standard for safety-critical adjacent systems |

---

## 11. Revision History

| Version | Date       | Description    |
|---------|------------|----------------|
| 1.0.0   | 2026-07-03 | Initial draft  |

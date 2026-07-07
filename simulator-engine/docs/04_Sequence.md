# MuST Replay Simulator Service — Sequence Diagrams

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-SIM-SEQ-004                         |
| **Version**        | 1.0.0-DRAFT                             |
| **Date**           | 2026-07-03                               |
| **Status**         | DRAFT — PENDING REVIEW                   |

---

## 1. Load File Sequence

This is the initialization sequence. An operator loads a telemetry file, which transitions the system from IDLE to READY.

```mermaid
sequenceDiagram
    participant OP as Operator
    participant REST as REST API
    participant CMD as Command Handler
    participant ORCH as Orchestrator
    participant FSM as State Machine
    participant SRC as SourcePort<br/>(FileReader)
    participant EVT as EventPort

    OP->>REST: POST /load {file_path, file_type}
    REST->>CMD: LoadFile command
    CMD->>CMD: Validate request fields
    CMD->>ORCH: LoadFile(path, type)
    ORCH->>FSM: can_transition(LOAD)?
    FSM-->>ORCH: Yes (current: IDLE)
    ORCH->>SRC: open(path)
    SRC->>SRC: Validate file exists
    SRC->>SRC: Read & validate headers
    SRC->>SRC: Compute metadata
    SRC-->>ORCH: Ok(SourceMetadata)
    ORCH->>FSM: transition(READY)
    FSM-->>ORCH: Ok(READY)
    ORCH->>EVT: emit(StatusChanged{IDLE→READY})
    ORCH-->>CMD: Ok(LoadResponse)
    CMD-->>REST: 200 {status: READY, file: ...}
    REST-->>OP: Response
```

**Design Rationale:**
- File validation happens inside the SourcePort adapter, not in the domain. WHY: the domain does not know what constitutes a valid binary vs. CCSDS file.
- State transition happens AFTER successful file open. WHY: if the file is invalid, we must stay in IDLE, not transition and then fail.
- Metadata is computed eagerly. WHY: operators need packet count and duration estimates in the load response.

---

## 2. Start Playback Sequence

Transitions from READY to RUNNING and begins the packet scheduling loop.

```mermaid
sequenceDiagram
    participant OP as Operator
    participant REST as REST API
    participant CMD as Command Handler
    participant ORCH as Orchestrator
    participant FSM as State Machine
    participant TE as Timing Engine
    participant SCHED as Scheduler Task
    participant SRC as SourcePort
    participant PUB as PublishPort
    participant EVT as EventPort
    participant MET as MetricsPort

    OP->>REST: POST /start {speed: 1.0}
    REST->>CMD: Start command
    CMD->>ORCH: Start(speed=1.0)
    ORCH->>FSM: can_transition(START)?
    FSM-->>ORCH: Yes (current: READY)
    ORCH->>TE: initialize(speed=1.0)
    ORCH->>FSM: transition(RUNNING)
    ORCH->>EVT: emit(PlaybackStarted)
    ORCH->>ORCH: spawn Scheduler Task
    
    loop Packet Scheduling Loop
        SCHED->>SRC: read_next_packet()
        SRC-->>SCHED: Ok(Some(ReplayPacket))
        SCHED->>TE: compute_delay(packet.timestamp)
        TE-->>SCHED: delay_duration
        SCHED->>SCHED: tokio::time::sleep(delay)
        SCHED->>PUB: publish(TelemetryEnvelope)
        PUB-->>SCHED: Ok
        SCHED->>MET: record_packets_published(1)
        SCHED->>EVT: emit(PacketPublished) [if subscribed]
    end
    
    SCHED->>SRC: read_next_packet()
    SRC-->>SCHED: Ok(None) [EOF]
    SCHED->>ORCH: notify(EOF)
    ORCH->>FSM: transition(COMPLETED)
    ORCH->>EVT: emit(PlaybackFinished)
    ORCH-->>CMD: Ok(StartResponse)
    CMD-->>REST: 200 {status: RUNNING}
    REST-->>OP: Response
```

**Design Rationale:**
- The Scheduler runs as a separate Tokio task. WHY: the REST response returns immediately after spawning. The operator does not wait for playback to complete.
- `tokio::time::sleep` is used for inter-packet delay. WHY: it yields the task, allowing other async work (command handling) to proceed.
- PacketPublished events are optional (subscriber-gated). WHY: at 100K pkt/s, unconditional event emission would overwhelm the event system.

---

## 3. Pause / Resume Sequence

```mermaid
sequenceDiagram
    participant OP as Operator
    participant ORCH as Orchestrator
    participant FSM as State Machine
    participant TE as Timing Engine
    participant SCHED as Scheduler Task
    participant EVT as EventPort

    Note over OP,EVT: === PAUSE ===
    OP->>ORCH: Pause()
    ORCH->>FSM: can_transition(PAUSE)?
    FSM-->>ORCH: Yes (current: RUNNING)
    ORCH->>TE: freeze()
    TE->>TE: Record pause_instant
    ORCH->>SCHED: signal(PAUSE)
    SCHED->>SCHED: Cancel pending sleep
    SCHED->>SCHED: Enter wait loop
    ORCH->>FSM: transition(PAUSED)
    ORCH->>EVT: emit(PlaybackPaused)

    Note over OP,EVT: === RESUME ===
    OP->>ORCH: Resume()
    ORCH->>FSM: can_transition(RESUME)?
    FSM-->>ORCH: Yes (current: PAUSED)
    ORCH->>TE: unfreeze()
    TE->>TE: Compute pause_duration
    TE->>TE: Offset session_start
    ORCH->>SCHED: signal(RESUME)
    SCHED->>SCHED: Exit wait loop
    SCHED->>SCHED: Recompute remaining delay
    ORCH->>FSM: transition(RUNNING)
    ORCH->>EVT: emit(PlaybackResumed)
```

**Design Rationale:**
- Pause cancels the current sleep rather than waiting for it to complete. WHY: instant pause response. If a sleep was 30 seconds (low-rate data), the operator would wait 30 seconds otherwise.
- The remaining delay for the current packet is recomputed on resume. WHY: if we paused 5s into a 10s sleep, we must only sleep 5s more, not restart the full 10s.

---

## 4. Seek Sequence

```mermaid
sequenceDiagram
    participant OP as Operator
    participant ORCH as Orchestrator
    participant FSM as State Machine
    participant TE as Timing Engine
    participant SRC as SourcePort
    participant EVT as EventPort

    OP->>ORCH: Seek(target_timestamp)
    ORCH->>FSM: can_seek()?
    FSM-->>ORCH: Yes (current: PAUSED)
    ORCH->>ORCH: Validate timestamp in range
    ORCH->>SRC: seek(target_timestamp)
    SRC->>SRC: Binary search packet index
    SRC->>SRC: Position file reader
    SRC-->>ORCH: Ok
    ORCH->>TE: reset(target_timestamp)
    TE->>TE: replay_clock = target
    TE->>TE: Clear drift accumulators
    TE->>TE: Reset session_start
    ORCH->>EVT: emit(StatusChanged)
    ORCH-->>OP: Ok(SeekResponse)
```

**Design Rationale:**
- Seek is only allowed in PAUSED, READY, or STOPPED states. WHY: seeking while RUNNING would create a race condition between the scheduler reading packets and the seek repositioning the reader.
- The timing engine performs a full reset. WHY: drift state from before the seek is meaningless after the discontinuity.
- The SourcePort uses binary search for seek. WHY: linear scan of a 64 GB file would be unacceptably slow. Packet timestamps are monotonically increasing (invariant), enabling binary search.

---

## 5. Error Recovery Sequence

```mermaid
sequenceDiagram
    participant SCHED as Scheduler Task
    participant SRC as SourcePort
    participant ORCH as Orchestrator
    participant FSM as State Machine
    participant EVT as EventPort
    participant MET as MetricsPort

    Note over SCHED,MET: === Recoverable Error (Corrupted Packet) ===
    SCHED->>SRC: read_next_packet()
    SRC-->>SCHED: Err(InvalidCcsdsHeader{offset})
    SCHED->>SCHED: Classify: Recoverable
    SCHED->>MET: record_error("invalid_ccsds_header")
    SCHED->>EVT: emit(PlaybackError{recoverable: true})
    SCHED->>SCHED: Skip packet, continue loop

    Note over SCHED,MET: === Unrecoverable Error (File I/O) ===
    SCHED->>SRC: read_next_packet()
    SRC-->>SCHED: Err(IoError{...})
    SCHED->>SCHED: Classify: Unrecoverable
    SCHED->>ORCH: notify(FatalError)
    ORCH->>FSM: transition(ERROR)
    ORCH->>EVT: emit(PlaybackError{recoverable: false})
    ORCH->>EVT: emit(StatusChanged{RUNNING→ERROR})
    SCHED->>SCHED: Exit loop
```

**Design Rationale:**
- Error classification happens at the boundary (Scheduler), not in the adapter. WHY: the same adapter error (e.g., read failure) might be recoverable in one context (single bad sector) but unrecoverable in another (disk offline).
- Recoverable errors do not involve the orchestrator. WHY: the scheduler handles them locally to avoid synchronization overhead on the hot path.

---

## 6. Full Packet Flow Sequence (Hot Path)

This is the steady-state packet flow during RUNNING. This path executes for every single packet.

```mermaid
sequenceDiagram
    participant FILE as Telemetry File
    participant FR as FileReader<br/>(SourcePort)
    participant SCHED as Replay Scheduler
    participant TE as Timing Engine
    participant PUB as GrpcPublisher<br/>(PublishPort)
    participant GW as Telemetry Gateway<br/>(Downstream)

    SCHED->>FR: read_next_packet()
    FR->>FILE: buffered read
    FILE-->>FR: raw bytes
    FR->>FR: Frame packet (sync/header)
    FR->>FR: Extract timestamp
    FR->>FR: Validate structure
    FR-->>SCHED: ReplayPacket{ts, data, offset}
    
    SCHED->>TE: compute_delay(packet.ts)
    TE->>TE: delta = packet.ts - prev_ts
    TE->>TE: adjusted = delta / speed
    TE->>TE: Apply drift correction
    TE-->>SCHED: Duration(adjusted)
    
    SCHED->>SCHED: tokio::time::sleep(adjusted)
    Note over SCHED: Yields to Tokio runtime
    
    SCHED->>SCHED: Build TelemetryEnvelope
    SCHED->>PUB: publish(envelope)
    PUB->>GW: gRPC stream.send(envelope)
    GW-->>PUB: Ok (flow control)
    PUB-->>SCHED: Ok
    
    SCHED->>SCHED: Increment counters
```

**Performance Note:** This sequence executes up to 100,000 times per second at 32x speed. Every allocation, lock, and system call on this path must be justified.

---

## 7. Startup Sequence

```mermaid
sequenceDiagram
    participant PROC as Process
    participant CFG as Config Loader
    participant LOG as Tracing Setup
    participant MET as Metrics Setup
    participant DI as Dependency Assembly
    participant REST as Axum Server
    participant GRPC as Tonic Server
    participant HEALTH as Health Probes

    PROC->>CFG: Load YAML + env overrides
    CFG-->>PROC: AppConfig
    PROC->>LOG: Initialize tracing subscriber
    PROC->>MET: Initialize Prometheus registry
    PROC->>DI: Create adapters
    DI->>DI: FileReaderAdapter::new()
    DI->>DI: GrpcPublisherAdapter::new()
    DI->>DI: EventBusAdapter::new()
    DI->>DI: PrometheusAdapter::new()
    DI->>DI: ReplayOrchestrator::new(adapters)
    PROC->>HEALTH: Set startup = ready
    PROC->>REST: Spawn Axum on port 8080
    PROC->>GRPC: Spawn Tonic on port 50051
    PROC->>PROC: await shutdown signal (SIGTERM/SIGINT)
```

---

## 8. Shutdown Sequence

```mermaid
sequenceDiagram
    participant SIG as SIGTERM
    participant PROC as Process
    participant ORCH as Orchestrator
    participant SCHED as Scheduler Task
    participant SRC as SourcePort
    participant PUB as PublishPort
    participant REST as Axum Server
    participant GRPC as Tonic Server

    SIG->>PROC: Signal received
    PROC->>ORCH: shutdown()
    ORCH->>SCHED: signal(STOP)
    SCHED->>SCHED: Exit loop
    ORCH->>SRC: close()
    ORCH->>PUB: flush & close
    PROC->>REST: graceful_shutdown()
    PROC->>GRPC: graceful_shutdown()
    REST-->>PROC: Drained
    GRPC-->>PROC: Drained
    PROC->>PROC: Exit 0
```

**Design Rationale:**
- Graceful shutdown ensures in-flight packets are published before exit. WHY: abrupt termination could leave the downstream gateway with an incomplete stream.
- Source is closed to release file handles. WHY: leaked file handles in a containerized environment can exhaust the PID's fd limit on restart.

---

## 9. Revision History

| Version | Date       | Description    |
|---------|------------|----------------|
| 1.0.0   | 2026-07-03 | Initial draft  |

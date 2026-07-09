# MuST Telemetry Gateway — State Machines

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-GW-STATE-005                        |
| **Version**        | 1.0.0-DRAFT                             |
| **Date**           | 2026-07-03                               |
| **Status**         | DRAFT — PENDING REVIEW                   |

---

## 1. Source Lifecycle State Machine

A telemetry source object tracks connectivity and operational registry status. In Version 1, registration is static and loaded at startup.

```mermaid
stateDiagram-v2
    [*] --> REGISTERED : Service Startup (static configuration)
    REGISTERED --> CONNECTED : Source client connects (gRPC StreamTelemetry)
    CONNECTED --> REGISTERED : Session finished / EOF or connection lost
    REGISTERED --> [*] : Service Shutdown
```

### Transition Table

| Current State | Event | Target State | Action / Side Effect |
|---------------|-------|--------------|----------------------|
| `[*]` | Service Startup | `REGISTERED` | Load static configs from environment / file |
| `REGISTERED` | Client Connect | `CONNECTED` | Open gRPC stream, start ingestion session |
| `CONNECTED` | Session Finished (EOF) / Disconnect | `REGISTERED` | Print session report, close stream |
| `REGISTERED` | Service Shutdown | `[*]` | Cleanup resources |

---

## 2. Telemetry Session State Machine

A session encapsulates a single, continuous stream of telemetry from a connected source.

```mermaid
stateDiagram-v2
    [*] --> ACTIVE : Stream opened
    ACTIVE --> BACKPRESSURE_BLOCKED : Buffer saturation (RabbitMQ lag)
    BACKPRESSURE_BLOCKED --> ACTIVE : Buffer cleared
    ACTIVE --> COMPLETED : Source sends EOF
    ACTIVE --> FAILED : Connection lost
    COMPLETED --> [*]
    FAILED --> [*]
```

### Transition Table

| Current State | Event | Target State | Action / Side Effect |
|---------------|-------|--------------|----------------------|
| `[*]` | Stream Opened | `ACTIVE` | Initialize session stats, reset silence timers |
| `ACTIVE` | Socket Write Blocked | `BACKPRESSURE_BLOCKED` | Block ingestion loop, triggering downstream TCP/HTTP2 flow control |
| `BACKPRESSURE_BLOCKED` | Socket Writable | `ACTIVE` | Resume publishing |
| `ACTIVE` | EOF message received | `COMPLETED` | Finalize statistics, print verification report |
| `ACTIVE` | Stream disconnected abruptly | `FAILED` | Mark session failed, print verification report |

---

## 3. Gateway Service State Machine

Represents the global system state of the service instance.

```mermaid
stateDiagram-v2
    [*] --> STARTING : Process init
    STARTING --> INITIALIZED : Port binding & config load successful
    INITIALIZED --> RUNNING : RabbitMQ connection successful
    RUNNING --> DEGRADED : RabbitMQ connection lost (buffering mode)
    DEGRADED --> RUNNING : RabbitMQ reconnected
    RUNNING --> SHUTTING_DOWN : SIGTERM / SIGINT
    DEGRADED --> SHUTTING_DOWN : SIGTERM / SIGINT
    SHUTTING_DOWN --> [*] : Resource cleanup complete
```

### Invariants
- **Telemetry Ingestion Invariant**: In the `DEGRADED` state, telemetry continues to be accepted and written to stdout (Console Sink fallback) to prevent packet loss.
- **System Memory Invariant**: Under no circumstance shall the gateway grow its buffers beyond memory safety boundaries. All operations rely on small, fixed-size heap structures.

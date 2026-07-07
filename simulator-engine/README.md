# MuST Replay Simulator Service

> **Multi-Station Telemetry & Tracking System — Replay Simulator Engine**

## Overview

The Replay Simulator Service (RSS) is a development-critical subsystem of MuST that simulates a live telemetry receiver. It reads recorded telemetry files and streams packets to downstream services with timing fidelity indistinguishable from a live ground station.

**This service is the ONLY telemetry source during development.** When the real receiver is integrated, it implements the same `SourcePort` interface — zero downstream changes required.

## Architecture

- **Pattern:** Hexagonal Architecture (Ports & Adapters)
- **Language:** Rust (edition 2021+)
- **Runtime:** Tokio
- **APIs:** REST (Axum) + gRPC (Tonic)
- **Serialization:** Protocol Buffers
- **Observability:** tracing (logs) + Prometheus (metrics)

## Documentation

All design documentation must be reviewed and approved before implementation begins.

| # | Document | Description |
|---|----------|-------------|
| 01 | [SRS](docs/01_SRS.md) | Software Requirements Specification — functional, non-functional, interface requirements |
| 02 | [Architecture](docs/02_Architecture.md) | Hexagonal design, component architecture, packet flow, timing engine, project structure |
| 03 | [API](docs/03_API.md) | REST endpoints, gRPC service definition, protobuf messages, error codes |
| 04 | [Sequence Diagrams](docs/04_Sequence.md) | Load, start, pause/resume, seek, error recovery, packet flow sequences |
| 05 | [State Machine](docs/05_StateMachine.md) | FSM states, transition table, command specifications, invariant verification |
| 06 | [Deployment](docs/06_Deployment.md) | Docker, configuration, health probes, metrics, logging, security |
| 07 | [Test Plan](docs/07_TestPlan.md) | Unit, integration, E2E test cases with traceability, CI pipeline |
| 08 | [Acceptance Criteria](docs/08_Acceptance.md) | 12 acceptance criteria groups with sign-off workflow |

## Project Structure

```
simulator-engine/
├── docs/           # Design documentation (8 documents)
├── proto/          # Protobuf definitions (API-first)
├── src/
│   ├── domain/     # Pure business logic (no I/O)
│   ├── application/# Use case orchestration
│   ├── ports/      # Trait definitions (contracts)
│   ├── adapters/   # Concrete implementations
│   │   ├── inbound/  # REST + gRPC (driving)
│   │   └── outbound/ # File reader, publisher (driven)
│   ├── config/     # YAML + env configuration
│   └── telemetry/  # Logging + metrics setup
├── configs/        # Environment-specific YAML configs
├── tests/          # Integration tests + fixtures
├── scripts/        # Utility scripts
├── Dockerfile
├── docker-compose.yml
└── README.md
```

## State Machine

```
IDLE ──LOAD──> READY ──START──> RUNNING ──PAUSE──> PAUSED
                 ▲                  │                  │
                 │                  │ EOF              │ RESUME
                 │                  ▼                  │
               LOAD           COMPLETED <─────────────┘
                 │                  │
              STOPPED <──STOP──  RUNNING
                 │
              UNLOAD ──> IDLE

              (Any) ──error──> ERROR ──LOAD/UNLOAD──> READY/IDLE
```

## Quick Start (Post-Implementation)

```bash
# Build
cargo build --release

# Run with config
./target/release/replay-simulator --config configs/default.yaml

# Docker
docker compose up -d

# Load a file
curl -X POST http://localhost:8080/api/v1/replay/load \
  -H "Content-Type: application/json" \
  -d '{"file_path": "pass_2026_07_03.bin", "file_type": "binary"}'

# Start playback
curl -X POST http://localhost:8080/api/v1/replay/start \
  -d '{"speed": 1.0}'

# Check status
curl http://localhost:8080/api/v1/replay/status
```

## Development Methodology

This project follows **Specification-Driven Development (SDD)**:

1. All 8 design documents are written and approved first
2. Protobuf API definitions are written before Rust code
3. Test cases are defined before implementation
4. Implementation follows the hexagonal architecture strictly
5. Acceptance criteria are verified before merge

## License

Internal — MuST Project

# MuST Telemetry Gateway

The **Telemetry Gateway** is the single ingestion point for all incoming telemetry streams in the
**MuST (Multi-Station Telemetry & Tracking System)** platform.

It receives CCSDS telemetry packets from the Replay Simulator over gRPC, runs the domain pipeline
(Validate → Normalize → Enrich → Route), and publishes enriched envelopes to a RabbitMQ topic
exchange using publisher confirms.

---

## Architecture

```
Replay Simulator (gRPC client)
        │
        │  StreamTelemetry  (port 50052)
        ▼
┌─────────────────────────────────────┐
│         Telemetry Gateway           │
│                                     │
│  TelemetryIngressServiceAdapter     │  ← gRPC inbound adapter (Tonic)
│           │                         │
│           ▼                         │
│   IngestionOrchestrator             │  ← application layer
│     1. Normalizer                   │
│     2. Validator                    │
│     3. Enricher                     │
│     4. Router  →  routing key       │
│           │                         │
│           ▼                         │
│   RabbitMqPublisherAdapter          │  ← outbound adapter (Lapin)
│   (falls back to console if down)   │
└─────────────────────────────────────┘
        │
        │  AMQP publish  (exchange: telemetry.raw)
        │  routing key:  cy3.sat101.{apid}.raw
        ▼
    RabbitMQ broker
```

---

## Project Structure

```
telemetry-gateway/
├── build.rs                        # tonic-build protobuf compilation
├── Cargo.toml
├── docs/                           # Architecture, SRS, State Machine, Error Handling
└── src/
    ├── api.rs                      # Generated protobuf modules (tonic::include_proto!)
    ├── main.rs                     # Entry point — wires all adapters
    ├── domain/
    │   ├── errors.rs               # GatewayError enum
    │   ├── enricher.rs             # Timestamps, UUID, metadata resolution
    │   ├── normalizer.rs           # String sanitisation
    │   ├── router.rs               # Routing key builder
    │   ├── validator.rs            # Payload & session validation
    │   └── models.rs               # SourceRegistration, Session
    ├── ports/
    │   ├── inbound/
    │   │   ├── ingest_port.rs      # IngestPort trait
    │   │   └── control_port.rs     # ControlPort trait
    │   └── outbound/
    │       ├── publish_port.rs     # PublishPort trait
    │       └── event_port.rs       # EventPort trait
    ├── adapters/
    │   ├── inbound/
    │   │   └── grpc/
    │   │       └── replay_receiver.rs  # TelemetryIngressService impl (Tonic server)
    │   └── outbound/
    │       └── rabbitmq/
    │           └── publisher.rs    # RabbitMqPublisherAdapter (Lapin)
    └── application/
        └── orchestrator.rs         # IngestionOrchestrator — domain pipeline + stats
```

---

## Prerequisites

| Tool    | Version | Install |
|---------|---------|---------|
| Rust    | ≥ 1.78  | `curl https://sh.rustup.rs -sSf \| sh` |
| protoc  | ≥ 3.21  | bundled at `/home/admin-yash/Desktop/Decode/bin/bin/protoc` |
| Docker  | any     | for RabbitMQ |
| curl    | any     | for REST commands to simulator |

---

## How to Run the Full Pipeline

Run each step in order. Steps 2 and 3 each need their **own terminal window**.

---

### Step 0 — (If restarting) Free port 50052

If you see `Address already in use` when starting the gateway, run this first:

```bash
# Any terminal — no specific directory required
lsof -ti :50052 | xargs -r kill -9
```

---

### Step 1 — Start RabbitMQ

```bash
# Any terminal — no specific directory required
docker run -d --name rabbitmq-must \
  -p 5672:5672 \
  -p 15672:15672 \
  -e RABBITMQ_DEFAULT_USER=guest \
  -e RABBITMQ_DEFAULT_PASS=guest \
  rabbitmq:3.13-management
```

Wait ~10 seconds for the broker to be ready.  
Management UI → http://localhost:15672 (guest / guest)

> If the container already exists from a previous run:
> ```bash
> docker start rabbitmq-must
> ```

---

### Step 2 — Start the Telemetry Gateway (Terminal 1)

```bash
# Directory: /home/admin-yash/Desktop/Decode/telemetry-gateway
cd /home/admin-yash/Desktop/Decode/telemetry-gateway
PROTOC=/home/admin-yash/Desktop/Decode/bin/bin/protoc cargo run
```

**Expected output:**
```
INFO  Initializing Telemetry Gateway Service (Sprint 3)...
INFO  RabbitMQ target: amqp://guest:guest@127.0.0.1:5672/%2f
INFO  gRPC Ingress Server listening on 0.0.0.0:50052
INFO  RabbitMQ: connection established.
INFO  RabbitMQ: channel ready, exchange 'telemetry.raw' declared.
```

> The gateway retries RabbitMQ every 5 s automatically.
> If RabbitMQ is not up yet, it falls back to console logging — no crash.

Leave this terminal running.

---

### Step 3 — Start the Replay Simulator (Terminal 2)

```bash
# Directory: /home/admin-yash/Desktop/Decode/simulator-engine
cd /home/admin-yash/Desktop/Decode/simulator-engine
cargo run --bin simulator-engine
```

**Expected output:**
```
INFO  Initializing MuST Replay Simulator Service...
INFO  Successfully loaded configuration from: configs/default.yaml
INFO  Connecting to telemetry gateway at http://127.0.0.1:50052...
INFO  Telemetry gateway connected successfully!
INFO  Starting REST API server on http://0.0.0.0:8080...
INFO  Starting gRPC API server on 0.0.0.0:50051...
```

Leave this terminal running.

---

### Step 4 — Load a Telemetry File

```bash
# Any terminal — no specific directory required
curl -X POST http://localhost:8080/api/v1/replay/load \
  -H "Content-Type: application/json" \
  -d '{"file_path": "data/sample.ccsds", "file_type": "ccsds"}'
```

**Expected response:**
```json
{
  "status": "READY",
  "file": {
    "path": "data/sample.ccsds",
    "size_bytes": 22000,
    "estimated_packets": 1000,
    "file_type": "ccsds"
  }
}
```

---

### Step 5 — Start Replay

```bash
# Any terminal — no specific directory required
curl -X POST http://localhost:8080/api/v1/replay/start \
  -H "Content-Type: application/json" \
  -d '{"speed": 1.0}'
```

Use `"speed": 32.0` for fast replay during development.

**Gateway output per packet (Terminal 1):**
```
INFO  [RabbitMQ ✓] key=cy3.sat101.42.raw | EnvID=... | Seq=1 | APID=42 | 22 bytes
```

---

### Step 6 — Read the Verification Report

After all 1000 packets are sent, the gateway automatically prints:

```
==================================================
   REPLAY VERIFICATION REPORT (SPRINT 3)
==================================================
 Received  : 1000
 Published : 1000
 Dropped   : 0
--------------------------------------------------
 Latency (Origin → Gateway Publish):
   Avg     : X.XXX ms
   Min     : X.XXX ms
   Max     : X.XXX ms
   Jitter  : X.XXX ms
--------------------------------------------------
 Avg Queue Delay : 0.005 ms
==================================================
```

---

### Step 7 — Verify in RabbitMQ Management UI

1. Open → http://localhost:15672
2. Login → guest / guest
3. Go to **Exchanges** tab → click `telemetry.raw`
4. Confirm message rate was non-zero during replay

---

## Other Simulator REST Commands

```bash
# Pause playback
curl -X POST http://localhost:8080/api/v1/replay/pause

# Resume playback
curl -X POST http://localhost:8080/api/v1/replay/resume

# Stop playback
curl -X POST http://localhost:8080/api/v1/replay/stop

# Change speed mid-replay
curl -X POST http://localhost:8080/api/v1/replay/speed \
  -H "Content-Type: application/json" \
  -d '{"speed": 4.0}'

# Check current status
curl http://localhost:8080/api/v1/replay/status

# Health check
curl http://localhost:8080/health/live
```

---

## Stop Everything

```bash
# Stop and remove RabbitMQ container
docker stop rabbitmq-must && docker rm rabbitmq-must

# Stop Gateway  →  Ctrl+C in Terminal 1
# Stop Simulator →  Ctrl+C in Terminal 2
```

---

## Documentation

| File | Contents |
|------|----------|
| `docs/02_Architecture.md` | Hexagonal architecture, concurrency model, RabbitMQ integration |
| `docs/03_State_Machine.md` | Gateway states: STARTING → READY → STREAMING → DRAINING |
| `docs/04_Error_Handling.md` | RabbitMQ retry strategy, backpressure chain, dead-letter queue |

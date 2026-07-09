# MuST Telemetry Pipeline (Version 1)

Welcome to the **Multi-Station Telemetry & Tracking (MuST) Telemetry Pipeline** workspace. This repository contains the Version 1 implementation of the ingestion, validation, and decoding pipeline, built entirely in **Rust** using **Tonic** (gRPC), **Lapin** (AMQP/RabbitMQ), and a strict **Hexagonal Architecture**.

---

## 1. Pipeline Overview

The telemetry pipeline processes satellite telemetry through sequential stages, progressively validating and enriching the data:

```
┌──────────────────┐               ┌──────────────────┐               ┌──────────────────┐               ┌──────────────────┐
│                  │               │                  │               │                  │               │                  │
│ Replay Simulator │ ──[gRPC]─────>│    Telemetry     │ ──[AMQP]─────>│  CCSDS Decoder   │ ──[AMQP]─────>│  Mission Ident.  │
│ (gRPC Client)    │               │  Ingress Gateway │               │  Service         │               │  Service         │
└──────────────────┘               └────────┬─────────┘               └────────┬─────────┘               └────────┬─────────┘
                                            │                                  │                                  │
                                            ▼ (telemetry.raw)                  ▼ (telemetry.decoded)              ▼ (telemetry.identified)
                                     [RabbitMQ Exchange]                [RabbitMQ Exchange]                [RabbitMQ Exchange]
```

1. **Replay Simulator**: Reads raw binary or CCSDS packet files, simulates telemetry replay clocks, and streams envelopes as a gRPC client.
2. **Telemetry Gateway**: Acts as the single ingress normalization boundary. It validates packets, stamps system-authoritative timestamps, and publishes to the `telemetry.raw` exchange.
3. **CCSDS Decoder Service**: Consumes from the `telemetry.raw` bus queue, decodes CCSDS primary and secondary header fields, runs sequence continuity validation per APID, and publishes the enriched envelopes to `telemetry.decoded`.
4. **Mission Identification Service (MIS)**: Consumes from the `telemetry.decoded` bus queue, maps incoming packets to specific missions and spacecraft registries using a rule lookup engine, and publishes to `telemetry.identified`.

---

## 2. Directory Layout & Documentation

- **[`architecture/`](file:///home/admin-yash/Desktop/Decode/architecture/)**: System-wide design contracts and architectural decisions.
  - [`02_Shared_Contracts.md`](file:///home/admin-yash/Desktop/Decode/architecture/02_Shared_Contracts.md): Protobuf envelope schemas.
  - [`03_Message_Bus_Design.md`](file:///home/admin-yash/Desktop/Decode/architecture/03_Message_Bus_Design.md): RabbitMQ exchange/queue and routing key topology.
  - [`04_MuST_System_Architecture.md`](file:///home/admin-yash/Desktop/Decode/architecture/04_MuST_System_Architecture.md): System data flow and architectural design.
- **[`telemetry-gateway/`](file:///home/admin-yash/Desktop/Decode/telemetry-gateway/)**: Normalization and ingestion gateway.
  - Docs: [`SRS`](file:///home/admin-yash/Desktop/Decode/telemetry-gateway/docs/01_SRS.md) | [`Architecture`](file:///home/admin-yash/Desktop/Decode/telemetry-gateway/docs/02_Architecture.md) | [`Sequence`](file:///home/admin-yash/Desktop/Decode/telemetry-gateway/docs/04_Sequence.md) | [`Deployment`](file:///home/admin-yash/Desktop/Decode/telemetry-gateway/docs/06_Deployment.md)
- **[`ccsds-decoder/`](file:///home/admin-yash/Desktop/Decode/ccsds-decoder/)**: Downstream CCSDS framing and sequence validator.
  - Docs: [`SRS`](file:///home/admin-yash/Desktop/Decode/ccsds-decoder/docs/01_SRS.md) | [`Architecture`](file:///home/admin-yash/Desktop/Decode/ccsds-decoder/docs/02_Architecture.md) | [`Sequence`](file:///home/admin-yash/Desktop/Decode/ccsds-decoder/docs/03_Sequence.md) | [`Deployment`](file:///home/admin-yash/Desktop/Decode/ccsds-decoder/docs/04_Deployment.md)
- **[`mission-identification-service/`](file:///home/admin-yash/Desktop/Decode/mission-identification-service/)**: Downstream contextual router and enricher (design specs).
  - Docs: [`Vision & SRS`](file:///home/admin-yash/Desktop/Decode/mission-identification-service/docs/01_Vision_and_Requirements.md) | [`Architecture`](file:///home/admin-yash/Desktop/Decode/mission-identification-service/docs/02_Architecture.md) | [`Contracts & Flows`](file:///home/admin-yash/Desktop/Decode/mission-identification-service/docs/03_Contracts_and_DataFlow.md) | [`Operations & Testing`](file:///home/admin-yash/Desktop/Decode/mission-identification-service/docs/04_Operations_and_Testing.md)
- **[`simulator-engine/`](file:///home/admin-yash/Desktop/Decode/simulator-engine/)**: Teleplay replay simulator.
  - Docs: [`API Specification`](file:///home/admin-yash/Desktop/Decode/simulator-engine/docs/03_API.md)
- **[`shared/proto/`](file:///home/admin-yash/Desktop/Decode/shared/proto/)**: Common Protobuf definitions.

---

## 3. Quickstart & Integration Testing

### Step 1: Start RabbitMQ Broker
Ensure RabbitMQ is running locally:
```bash
docker run -d --name rabbitmq -p 5672:5672 -p 15672:15672 rabbitmq:3-management
```

### Step 2: Build and Run the Telemetry Gateway
```bash
cd telemetry-gateway
cargo run
```
*Listens on gRPC port `50052`.*

### Step 3: Build and Run the CCSDS Decoder Service
```bash
cd ccsds-decoder
AMQP_URL=amqp://guest:guest@127.0.0.1:5672/%2f cargo run
```

### Step 4: Build and Run the Mission Identification Service (future implementation)
```bash
cd mission-identification-service
AMQP_URL=amqp://guest:guest@127.0.0.1:5672/%2f cargo run
```

### Step 5: Run the Replay Simulator
```bash
cd simulator-engine
cargo run
```
*Listens on REST port `8081` and gRPC control port `50051`. Pushes telemetry to Gateway at `127.0.0.1:50052`.*

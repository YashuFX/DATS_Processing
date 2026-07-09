# CCSDS Decoder Service — Architecture Document

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-DEC-ARCH-002                        |
| **Version**        | 1.0.0                                    |
| **Date**           | 2026-07-09                               |
| **Status**         | APPROVED                                 |

---

## 1. Architectural Position

The CCSDS Decoder is an event-driven, pipeline-enriching microservice that consumes raw satellite frames from the `telemetry.raw` message bus, decodes the CCSDS Space Packet header structures, validates compliance, tracks sequence continuity, and publishes the enriched result to `telemetry.decoded`.

```
           Ingress Bus                       Egress Bus
        ┌──────────────┐                  ┌──────────────┐
        │  RabbitMQ    │                  │  RabbitMQ    │
        │  telemetry   │                  │  telemetry   │
        │    .raw      │                  │   .decoded   │
        └──────┬───────┘                  └──────▲───────┘
               │                                 │
               │ [consume]                       │ [publish]
        ┌──────▼─────────────────────────────────┴──────┐
        │                                               │
        │            CCSDS DECODER SERVICE              │
        │                                               │
        └───────────────────────────────────────────────┘
```

---

## 2. Hexagonal Architecture

The service is strictly built using **Hexagonal Architecture** principles, separating core application rules from I/O adapters, frameworks, and serialization details.

```
┌─────────────────────────────────────────────────────────────────────┐
│                    DRIVING ADAPTERS (Inbound)                       │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │ RabbitMqConsumer (lapin)                                     │  │
│  │ (Listens on queue, binds to routing key "#.raw")              │  │
│  └───────────────────────┬──────────────────────────────────────┘  │
│                          │                                          │
│                          ▼                                          │
│                  ┌───────────────┐                                  │
│                  │    PORTS      │ (EnvelopeConsumer, DeliveryAcker)│
│                  └───────┬───────┘                                  │
├──────────────────────────┼──────────────────────────────────────────┤
│                     APPLICATION CORE                                 │
│  ┌───────────────────────▼────────────────────────────────────────┐  │
│  │               DecoderOrchestrator                              │  │
│  │  ┌──────────────┐ ┌──────────────┐ ┌────────────────────┐      │  │
│  │  │ Parser       │ │ Validator    │ │ ContinuityEngine   │      │  │
│  │  └──────────────┘ └──────────────┘ └────────────────────┘      │  │
│  └───────────────────────┬────────────────────────────────────────┘  │
│                          │                                          │
│                  ┌───────▼───────┐                                  │
│                  │    PORTS      │ (DecodedPublisher, DecodedSink)  │
│                  └───────┬───────┘                                  │
├──────────────────────────┼──────────────────────────────────────────┤
│                    DRIVEN ADAPTERS (Outbound)                        │
│  ┌───────────────────────────────┬──────────────────────────────┐  │
│  │ RabbitMqPublisher (lapin)     │ ConsoleSink                  │  │
│  │ (Publishes to exchange)       │ (Prints packet summaries)    │  │
│  └───────────────────────────────┴──────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

### 2.1 Module Responsibilities

#### Inbound Ports & Adapters
- `EnvelopeConsumer`: Port trait for starting consumption of raw envelopes.
- `RabbitMqConsumer`: Inbound adapter using `lapin` to establish connection, create queues, and process raw bytes from `telemetry.raw`.
- `DeliveryAcker`: Inbound port wrapping message acknowledgements (ACK/NACK).

#### Application Core
- `DecoderOrchestrator`: The main use case orchestrator. It executes the step-by-step pipeline: deserializes incoming envelopes, delegates parsing and validation to domain classes, tracks continuity, mutates the envelope, and publishes/logs the result.

#### Domain Logic
- `Parser`: Decodes standard CCSDS primary headers (big-endian bit unpacking) and optional secondary headers.
- `Validator`: Enforces protocol requirements (minimum length, correct version, CRC check).
- `ContinuityEngine`: Maintains state under local mutex lock to trace sequence counts per APID, detecting gaps and duplicates.
- `CcsdsHeader` & `TimeCodeFormat`: Domain models for CCSDS packet values.

#### Outbound Ports & Adapters
- `DecodedPublisher`: Outbound port trait for publishing mutated envelopes.
- `RabbitMqPublisher`: Outbound adapter using `lapin` with publisher confirms enabled to send the enriched packet downstream.
- `DecodedSink`: Outbound port trait for logging/tracing.
- `ConsoleSink`: Outbound adapter logging summary data to standard output.

---

## 3. Technology Selection

- **Language**: Rust (2021 edition) for execution speed, zero memory safety overhead, and native multi-threading safety.
- **AMQP Broker Client**: `lapin` (pure Rust asynchronous AMQP client) for high performance and compatibility with Tokio.
- **Serialization**: `prost` for compiling and decoding Google Protocol Buffers (`TelemetryEnvelope` and nested types).
- **Concurrency**: `tokio` multi-threaded runtime.

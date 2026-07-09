# CCSDS Decoder Service — Sequence Diagram

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-DEC-SEQ-003                         |
| **Version**        | 1.0.0                                    |
| **Date**           | 2026-07-09                               |
| **Status**         | APPROVED                                 |

---

## 1. End-to-End Telemetry Processing Flow

The diagram below details the sequence of processing from the moment a message is consumed from `telemetry.raw` to its publish on `telemetry.decoded`.

```mermaid
sequenceDiagram
    autonumber
    participant RMQ_In as RabbitMQ (telemetry.raw)
    participant Consumer as RabbitMqConsumer
    participant Orch as DecoderOrchestrator
    participant Parser as Parser (Domain)
    participant Val as Validator (Domain)
    participant Engine as ContinuityEngine (Domain)
    participant Publisher as RabbitMqPublisher
    participant RMQ_Out as RabbitMQ (telemetry.decoded)
    participant Sink as ConsoleSink

    RMQ_In->>Consumer: Deliver AMQP Message (raw bytes)
    Consumer->>Orch: on_envelope_consumed(raw_bytes, routing_key)
    
    Note over Orch: Step 1: Deserialize Envelope
    Orch->>Orch: EnvelopeDeserializer::decode(raw_bytes)
    
    Note over Orch: Step 2: Extract raw_packet.data
    
    Orch->>Parser: Step 3: parse_primary_header(raw_data)
    Parser-->>Orch: CcsdsPrimaryHeader
    
    Orch->>Val: Step 4: validate_all(raw_data, primary_header, check_crc)
    Val-->>Orch: Result::Ok
    
    alt primary_header.sec_hdr_flag == true
        Orch->>Parser: Step 5: parse_secondary_header(raw_data, format)
        Parser-->>Orch: CcsdsSecondaryHeader
    end

    Note over Orch: Step 6: Continuity Check (Mutex Block)
    Orch->>Engine: check(apid, seq_count)
    Engine-->>Orch: ContinuityResult (is_gap, is_duplicate)

    Note over Orch: Step 7: Mutate TelemetryEnvelope in-place
    Note over Orch: Set stage = CCSDS_DECODED, decorate header / secondary, set quality, set publish_timestamp

    Note over Orch: Step 8: Publish decorated envelope to RabbitMQ
    Orch->>Publisher: publish(envelope, outbound_routing_key)
    Publisher->>RMQ_Out: AMQP basic_publish
    RMQ_Out-->>Publisher: basic_ack (Publisher Confirm)
    Publisher-->>Orch: Result::Ok

    Note over Orch: Step 9: Emit summary to Console Sink
    Orch->>Sink: emit(decode_result)
    Sink-->>Orch: Result::Ok
    
    Orch-->>Consumer: Result::Ok (Processing Succeeded)
    Consumer->>RMQ_In: basic_ack (Message Acknowledged)
```

---

## 2. Error and NACK Sequence

When parsing, validation, or network publishing fails, the message must be rejected (NACK'd) to prevent queue deadlocks and ensure no telemetry is lost.

```mermaid
sequenceDiagram
    autonumber
    participant RMQ_In as RabbitMQ (telemetry.raw)
    participant Consumer as RabbitMqConsumer
    participant Orch as DecoderOrchestrator
    participant Val as Validator (Domain)

    RMQ_In->>Consumer: Deliver AMQP Message (raw bytes)
    Consumer->>Orch: on_envelope_consumed(raw_bytes, routing_key)
    Orch->>Val: validate_all(...)
    Val-->>Orch: Err(DecoderError::CrcCheckFailed)
    Orch-->>Consumer: Err(DecoderError)
    Note over Consumer: Trigger Failure Handler
    Consumer->>RMQ_In: basic_nack (requeue = false)
    Note over RMQ_In: Routing to Dead Letter Queue (DLQ) if configured
```

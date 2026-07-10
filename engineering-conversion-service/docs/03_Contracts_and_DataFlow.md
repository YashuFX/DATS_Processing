# Engineering Conversion Service — Contracts and Data Flow

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-ECS-CON-003                        |
| **Version**        | 1.0.0                                    |
| **Date**           | 2026-07-10                               |
| **Status**         | PROPOSED                                 |

---

## 1. Protobuf Contract Updates

To support the injection of the Engineering Conversion Service into the telemetry pipeline, we define a new value in the shared `ProcessingStage` enum in `shared/proto/must/telemetry/v1/envelope.proto`.

### 1.1 Protobuf Enum Diff
```diff
// File: shared/proto/must/telemetry/v1/envelope.proto

 enum ProcessingStage {
   PROCESSING_STAGE_UNSPECIFIED = 0;
   PROCESSING_STAGE_RAW = 1;              // Raw bytes from source (Replay/Receiver)
   PROCESSING_STAGE_CCSDS_DECODED = 2;    // CCSDS headers parsed (CCSDS Service)
   PROCESSING_STAGE_ENGINEERING = 3;      // Engineering values extracted (XTCE Service)
   PROCESSING_STAGE_VALIDATED = 4;        // Limits checked (Validation Service)
   PROCESSING_STAGE_ARCHIVED = 5;         // Written to storage (Archive Service)
   PROCESSING_STAGE_IDENTIFIED = 6;        // Mission/satellite identified (Mission ID Service)
+  PROCESSING_STAGE_ENGINEERING_CONVERTED = 7; // Derived parameters computed (Conversion Service)
 }
```

---

## 2. RabbitMQ Topology & Contracts

The service acts as a middleware processor on the `telemetry.engineering` exchange. It ingests decommutated packets, processes them, and publishes them back to the same exchange with an updated routing key.

```
                  Exchange: telemetry.engineering (Topic)
                                │
                      [routing: *.decommutated]
                                ▼
                      Queue: engineering.convert
                                │
                    [Engineering Conversion]
                                │
                       [routing: *.engineering]
                                ▼
                  Exchange: telemetry.engineering (Topic)
```

### 2.1 Input Bindings
* **Exchange**: `telemetry.engineering` (topic, durable)
* **Queue**: `engineering.convert` (durable, configured with DLX `must.dlx`)
* **Routing Key Pattern**: `#.decommutated` (captures all decommutated envelopes from the XTCE Decoder)
* **QoS Prefetch**: `50` (optimized for parallel thread computation)

### 2.2 Output Bindings
* **Exchange**: `telemetry.engineering` (topic, durable)
* **Outbound Routing Key**: `{mission_code}.sat{satellite_id}.{apid}.engineering`
  * *Example*: `cy3.sat101.42.engineering`
  * *Note*: By publishing with the `.engineering` suffix, the Validation Service (bound to `#.engineering` on the same exchange) automatically ingests the message.

### 2.3 AMQP Message Properties
Every published envelope MUST declare these properties:
* `content_type`: `application/x-protobuf`
* `delivery_mode`: `2` (persistent)
* `message_id`: Matching `envelope.envelope_id` (preserves trace correlation)
* `app_id`: `engineering-conversion-service`

---

## 3. One Packet Journey Through Every Layer

Here is the step-by-step lifecycle of a single packet, focusing on how parameters are evaluated and enriched:

### 3.1 Step 1: Consumption and Deserialization
The service consumes an envelope from the `engineering.convert` queue.
* **Routing Key**: `cy3.sat101.42.decommutated`
* **Stage**: `PROCESSING_STAGE_ENGINEERING`
* **Parameters in Envelope**:
  1. Name: `/SC/EPS/BatteryVoltage`, Raw: `2755`, Engineering: `27.55` (Float), Validity: `Valid`
  2. Name: `/SC/EPS/BatteryCurrent`, Raw: `400`, Engineering: `4.0` (Float), Validity: `Valid`

### 3.2 Step 2: Configuration Lookup
The orchestrator reads the mission code `cy3` and performs a cache lookup in `FormulaRegistry`.
It retrieves `cy3.yaml` which defines the derived parameter rules:
```yaml
derived_parameters:
  - name: "/SC/EPS/BatteryPower"
    inputs:
      - parameter_name: "/SC/EPS/BatteryVoltage"
        alias: "v"
      - parameter_name: "/SC/EPS/BatteryCurrent"
        alias: "i"
    expression: "v * i"
```

### 3.3 Step 3: Expression Evaluation
The `ComputationEngine` resolves the inputs:
* Maps `/SC/EPS/BatteryVoltage` to alias `v` with value `27.55`
* Maps `/SC/EPS/BatteryCurrent` to alias `i` with value `4.0`
* Evaluates expression: `27.55 * 4.0` -> `110.2` (Float)
* Creates a new `TelemetryParameter`:
  * Name: `/SC/EPS/BatteryPower`
  * Raw Value: `None` (derived parameters have no raw telemetry source)
  * Engineering Value: `110.2` (Float)
  * Validity: `PARAMETER_VALIDITY_VALID`

### 3.4 Step 4: Envelope Enrichment and Publication
The orchestrator appends the new parameter to the envelope's parameters list (leaving the original parameters unchanged).
* **New Parameters List**:
  1. `/SC/EPS/BatteryVoltage` (27.55)
  2. `/SC/EPS/BatteryCurrent` (4.0)
  3. `/SC/EPS/BatteryPower` (110.2)
* **Stage Updated**: `PROCESSING_STAGE_ENGINEERING_CONVERTED`
* **Outbound Routing Key**: `cy3.sat101.42.engineering`
* **Egress**: Published to `telemetry.engineering` with manual ACK confirmation.

---

## 4. Sequence Diagram

```mermaid
sequenceDiagram
    autonumber
    participant Bus as RabbitMQ (telemetry.engineering)
    participant Consumer as RabbitMqConsumer
    participant Orch as ConversionOrchestrator
    participant Registry as FormulaRegistry (Cache)
    participant Engine as ComputationEngine (Domain)
    participant Publisher as RabbitMqPublisher

    Bus->>Consumer: Deliver AMQP Message (envelope bytes, key: *.decommutated)
    Consumer->>Orch: on_envelope_consumed(raw_bytes, routing_key)
    
    Note over Orch: Step 1: Deserialize Envelope
    Orch->>Orch: Deserializer::decode(raw_bytes)
    
    Note over Orch: Step 2: Retrieve Config File
    Orch->>Registry: get_db(mission_code)
    Registry-->>Orch: Arc<DerivedDb>
    
    Note over Orch: Step 3: Evaluate Derived Parameters
    loop For each DerivedParameterDefinition
        Orch->>Engine: evaluate(definition, envelope.parameters)
        Engine-->>Orch: TelemetryParameter
    end
    
    Note over Orch: Step 4: Mutate Envelope
    Note over Orch: Append parameters, set stage = ENGINEERING_CONVERTED
    
    Note over Orch: Step 5: Publish Enriched Message
    Orch->>Publisher: publish(envelope, key: *.engineering)
    Publisher->>Bus: AMQP basic_publish
    Bus-->>Publisher: Confirm ACK
    Publisher-->>Orch: Result::Ok
    
    Orch-->>Consumer: Result::Ok
    Consumer->>Bus: basic_ack (Confirm consumed message)
```

---

## 5. Component Diagram

```mermaid
graph TB
    subgraph "Inbound Adaption Layer"
        RMQ_C["RabbitMqConsumer"]
    end
    
    subgraph "Application Core"
        ORCH["ConversionOrchestrator"]
    end
    
    subgraph "Domain Core"
        ENGINE["ComputationEngine"]
        REG["FormulaRegistry (In-Memory Cache)"]
    end
    
    subgraph "Outbound Adaption Layer"
        RMQ_P["RabbitMqPublisher"]
        LOG["LoggingAlertPublisher"]
    end
    
    RMQ_C -->|delegates raw bytes| ORCH
    ORCH -->|gets formulas| REG
    ORCH -->|derived math math| ENGINE
    ORCH -->|sends enriched| RMQ_P
    ORCH -->|sends status/logs| LOG
```

---

## 6. Deployment Diagram

```mermaid
graph TB
    subgraph "Kubernetes Pod"
        direction TB
        BIN["Container: engineering-conversion-service<br/>(Rust Binary)"]
        CONFIG["Volume Mount:<br/>/etc/must/derived/<br/>(YAML Configuration Files)"]
        BIN <-->|Reads Config Files| CONFIG
    end
    
    subgraph "Infrastructure"
        RMQ["RabbitMQ Pod"]
        PROM["Prometheus Server"]
    end
    
    BIN <-->|AMQP TCP Connection| RMQ
    PROM -->|HTTP Scraping /metrics| BIN
```

---

## 7. Message State Diagram

```mermaid
stateDiagram-v2
    [*] --> CONSUMED : Message dequeued from telemetry.engineering
    
    CONSUMED --> DESERIALIZED : Protobuf deserialized successfully
    CONSUMED --> DLQ : Deserialization fails (NACK)
    
    DESERIALIZED --> CONFIG_RESOLVED : yaml config loaded from registry cache
    DESERIALIZED --> DLQ : Mission configuration not found (NACK)
    
    CONFIG_RESOLVED --> MATH_EVALUATED : Formulas evaluated
    CONFIG_RESOLVED --> ENRICHED_WITH_WARNINGS : Math errors / missing parameters (raw used, validity set to INVALID)
    
    MATH_EVALUATED --> MUTATED : Append parameters, set stage = ENGINEERING_CONVERTED
    ENRICHED_WITH_WARNINGS --> MUTATED : Append parameters (invalid state), set stage = ENGINEERING_CONVERTED
    
    MUTATED --> PUBLISHING : Publish confirms enabled
    
    PUBLISHING --> ACKNOWLEDGED : Publisher confirm received from exchange
    PUBLISHING --> RETRYING : Broker publish fails (Transient error)
    
    RETRYING --> MUTATED : Retry limit not exceeded
    RETRYING --> CRASH : Retry limit exceeded (FATAL)
    
    ACKNOWLEDGED --> [*] : manual basic_ack sent to consumer queue
    DLQ --> [*] : manual basic_nack sent to queue (routed to DLQ)
```

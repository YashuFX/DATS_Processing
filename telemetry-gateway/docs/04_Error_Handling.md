# Telemetry Gateway — Error Handling Specification

This document details how the **Telemetry Gateway** handles failures, connection losses, and resource exhaustion in production.

---

## 1. RabbitMQ Egress Failures

If the RabbitMQ broker becomes unreachable or fails to acknowledge a publish request, the system processes it as follows:

```
                  Publish Packet
                        │
                        ▼
                [RabbitMQ Live?] ──No──> [Egress Channel Full?]
                        │                          │
                       Yes                        Yes
                        │                          │
                        ▼                          ▼
               Publish to Broker           State = BACKPRESSURE
                        │                  Reject gRPC stream
                        │               (RESOURCE_EXHAUSTED)
              [Acknowledge Recvd?]
                 /            \
               Yes             No
               /                \
              ▼                  ▼
        Success (Confirm)   Retry Loop (100ms, 500ms)
                                 │
                            [Success?]
                             /       \
                           Yes        No
                           /           \
                          ▼             ▼
                    Success Confirm   Drop Packet & Dead Letter
```

### 1.1 Retry Strategy
* **Scope**: When a publish confirm is negative (nack) or times out.
* **Mechanism**:
  * **Attempt 1**: Immediate retry.
  * **Attempt 2**: Retry after `100ms` delay.
  * **Attempt 3**: Retry after `500ms` delay.
* **Failure Actions**: If all 3 attempts fail:
  1. Drop the packet from the active queue.
  2. Write the packet to the Dead Letter Queue (`gateway.dlq`) or log to error store.
  3. Emit a `PublishFailed` platform event with severity `ERROR`.

### 1.2 Buffering & Backpressure (When RabbitMQ is completely down)
* The gateway maintains a bounded publish queue (`tokio::sync::mpsc::channel`) with a capacity of **5,000** envelopes.
* **Behavior**:
  1. While the queue has capacity, the workers continue placing validated and normalized envelopes into the channel.
  2. If the queue fills up (exceeds 90% capacity, i.e., 4,500 envelopes), the gateway transitions to the `BACKPRESSURE` state.
  3. In the `BACKPRESSURE` state:
     * Inbound worker tasks block on `send()` to the publish channel.
     * The inbound `Ingestion Channel` (capacity 10,000) begins to fill.
     * Once the `Ingestion Channel` is full, the gRPC receiver rejects new incoming packets, returning `gRPC status: RESOURCE_EXHAUSTED` to the Replay Simulator.
     * The Replay Simulator, upon receiving `RESOURCE_EXHAUSTED`, triggers its internal timing scheduler pause strategy (backpressure).
  4. This chain ensures **data is buffered upstream in the simulator**, preventing the gateway from memory-exhaustion crashes.

---

## 2. Inbound gRPC Ingress Failures

| Failure Mode | Detection | Gateway Action |
|--------------|-----------|----------------|
| **Replay client disconnects abruptly** | Stream connection drop | Close active session, update state to `FAILED`, emit `SessionFailed` event. |
| **Duplicate Session ID** | Registry lookup on start | Reject connection with `ALREADY_EXISTS` status. |
| **Invalid Client Configuration** | Validation layer | Reject connection with `INVALID_ARGUMENT`. |

---

## 3. Auto-Reconnection & Recovery

* **RabbitMQ Connection Lost**:
  * The RabbitMQ publisher adapter maintains a background reconnect loop.
  * When a connection drops, it attempts to establish a new connection every **5 seconds**.
  * While reconnecting, the publish buffer remains intact (up to 5,000 envelopes) before entering the backpressure chain.

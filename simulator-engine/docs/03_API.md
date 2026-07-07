# MuST Replay Simulator Service — API Specification

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-SIM-API-003                         |
| **Version**        | 1.0.0-DRAFT                             |
| **Date**           | 2026-07-03                               |
| **Status**         | DRAFT — PENDING REVIEW                   |

---

## 1. API Design Philosophy

The RSS exposes two parallel APIs: REST (for human operators, dashboards, CI scripts) and gRPC (for service-to-service telemetry streaming). Both APIs share identical semantics and error codes.

**Why dual API:**
- REST is universal — `curl`, Postman, any HTTP client can control playback.
- gRPC is efficient — binary protobuf streaming for high-throughput telemetry delivery with backpressure.
- Downstream services consume gRPC streams. Operators use REST.

---

## 2. REST API

Base URL: `http://{host}:{port}/api/v1/replay`

### 2.1 POST /load

**Purpose:** Load a telemetry file for replay.

| Field | Value |
|-------|-------|
| Method | POST |
| Path | /api/v1/replay/load |
| Valid States | IDLE, STOPPED, COMPLETED |
| Target State | READY |

**Request Body:**
```json
{
  "file_path": "/data/telemetry/pass_2026_07_03.bin",
  "file_type": "binary",
  "timestamp_format": "epoch_ns"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| file_path | string | yes | Path to telemetry file (relative to configured base directory) |
| file_type | enum | yes | "binary", "ccsds", "pcap" |
| timestamp_format | enum | no | "epoch_ns" (default), "epoch_us", "ccsds_cuc" |

**Response 200:**
```json
{
  "status": "READY",
  "file": {
    "path": "/data/telemetry/pass_2026_07_03.bin",
    "size_bytes": 1073741824,
    "estimated_packets": 524288,
    "estimated_duration_seconds": 3600.5,
    "file_type": "binary"
  }
}
```

**Error Responses:**

| Code | Condition | Body |
|------|-----------|------|
| 400 | Missing required field | `{"error": "INVALID_REQUEST", "message": "file_path is required"}` |
| 404 | File not found | `{"error": "FILE_NOT_FOUND", "message": "..."}` |
| 409 | Invalid state (e.g., RUNNING) | `{"error": "INVALID_STATE", "message": "Cannot load while RUNNING. Stop first."}` |
| 422 | File validation failed | `{"error": "INVALID_FILE", "message": "CCSDS header validation failed at offset 0"}` |
| 500 | Internal error | `{"error": "INTERNAL", "message": "..."}` |

---

### 2.2 POST /start

**Purpose:** Begin playback from current position.

| Field | Value |
|-------|-------|
| Method | POST |
| Path | /api/v1/replay/start |
| Valid States | READY, STOPPED |
| Target State | RUNNING |

**Request Body:** (optional)
```json
{
  "speed": 1.0,
  "loop_enabled": false,
  "stop_at_timestamp": null
}
```

**Response 200:**
```json
{
  "status": "RUNNING",
  "speed": 1.0,
  "started_at": "2026-07-03T12:00:00Z"
}
```

| Code | Condition |
|------|-----------|
| 409 | Not in READY or STOPPED state |

---

### 2.3 POST /pause

**Purpose:** Pause playback, freezing the replay clock.

| Field | Value |
|-------|-------|
| Method | POST |
| Path | /api/v1/replay/pause |
| Valid States | RUNNING |
| Target State | PAUSED |

**Request Body:** None

**Response 200:**
```json
{
  "status": "PAUSED",
  "paused_at_packet": 12345,
  "paused_at_timestamp": 1720000000000000000
}
```

| Code | Condition |
|------|-----------|
| 409 | Not RUNNING |

---

### 2.4 POST /resume

**Purpose:** Resume playback from paused position.

| Field | Value |
|-------|-------|
| Method | POST |
| Path | /api/v1/replay/resume |
| Valid States | PAUSED |
| Target State | RUNNING |

**Request Body:** None

**Response 200:**
```json
{
  "status": "RUNNING",
  "resumed_at_packet": 12345
}
```

| Code | Condition |
|------|-----------|
| 409 | Not PAUSED |

---

### 2.5 POST /stop

**Purpose:** Stop playback. Position resets to beginning.

| Field | Value |
|-------|-------|
| Method | POST |
| Path | /api/v1/replay/stop |
| Valid States | RUNNING, PAUSED |
| Target State | STOPPED |

**Request Body:** None

**Response 200:**
```json
{
  "status": "STOPPED"
}
```

| Code | Condition |
|------|-----------|
| 409 | Not RUNNING or PAUSED |

---

### 2.6 POST /seek

**Purpose:** Jump to a specific timestamp within the loaded file.

| Field | Value |
|-------|-------|
| Method | POST |
| Path | /api/v1/replay/seek |
| Valid States | READY, PAUSED, STOPPED |
| Target State | (unchanged) |

**Request Body:**
```json
{
  "target_timestamp": 1720000000500000000,
  "mode": "absolute"
}
```

| Field | Type | Description |
|-------|------|-------------|
| target_timestamp | uint64 | Target timestamp in nanoseconds |
| mode | enum | "absolute" (file timestamp) or "relative" (offset from start) |

**Response 200:**
```json
{
  "status": "PAUSED",
  "seeked_to_packet": 25000,
  "seeked_to_timestamp": 1720000000500000000
}
```

| Code | Condition |
|------|-----------|
| 400 | Timestamp out of range |
| 409 | Cannot seek while RUNNING (pause first) |

---

### 2.7 POST /speed

**Purpose:** Change playback speed.

| Field | Value |
|-------|-------|
| Method | POST |
| Path | /api/v1/replay/speed |
| Valid States | READY, RUNNING, PAUSED, STOPPED |
| Target State | (unchanged) |

**Request Body:**
```json
{
  "speed": 4.0
}
```

| Field | Type | Allowed Values |
|-------|------|---------------|
| speed | float | 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 0.0 (step mode) |

**Response 200:**
```json
{
  "speed": 4.0,
  "previous_speed": 1.0
}
```

| Code | Condition |
|------|-----------|
| 400 | Invalid speed value |

---

### 2.8 POST /loop

**Purpose:** Enable or disable loop mode.

| Field | Value |
|-------|-------|
| Method | POST |
| Path | /api/v1/replay/loop |
| Valid States | Any except IDLE, ERROR |
| Target State | (unchanged) |

**Request Body:**
```json
{
  "enabled": true,
  "max_iterations": 0
}
```

| Field | Type | Description |
|-------|------|-------------|
| enabled | bool | Enable/disable loop |
| max_iterations | uint32 | 0 = infinite, N = stop after N loops |

**Response 200:**
```json
{
  "loop_enabled": true,
  "max_iterations": 0,
  "current_iteration": 0
}
```

---

### 2.9 GET /status

**Purpose:** Get current playback status and statistics.

| Field | Value |
|-------|-------|
| Method | GET |
| Path | /api/v1/replay/status |
| Valid States | Any |

**Response 200:**
```json
{
  "state": "RUNNING",
  "file": {
    "path": "/data/telemetry/pass_2026_07_03.bin",
    "size_bytes": 1073741824,
    "file_type": "binary"
  },
  "playback": {
    "speed": 4.0,
    "loop_enabled": false,
    "current_iteration": 0
  },
  "progress": {
    "packets_published": 125000,
    "total_packets_estimated": 524288,
    "frames_published": 125000,
    "progress_percent": 23.84,
    "elapsed_seconds": 225.0,
    "remaining_seconds": 718.5,
    "current_timestamp": 1720000000225000000,
    "file_offset_bytes": 256000000
  },
  "errors": {
    "packets_skipped": 3,
    "last_error": "Invalid CCSDS header at offset 128000000"
  }
}
```

---

### 2.10 GET /statistics

**Purpose:** Get detailed operational statistics.

| Field | Value |
|-------|-------|
| Method | GET |
| Path | /api/v1/replay/statistics |
| Valid States | Any |

**Response 200:**
```json
{
  "session": {
    "session_id": "a1b2c3d4",
    "started_at": "2026-07-03T12:00:00Z",
    "uptime_seconds": 3600
  },
  "throughput": {
    "packets_per_second_current": 347.2,
    "packets_per_second_average": 345.8,
    "bytes_per_second_current": 712294.4,
    "bytes_per_second_average": 709478.4
  },
  "timing": {
    "average_jitter_ns": 450,
    "max_jitter_ns": 2300,
    "drift_correction_count": 12,
    "total_drift_corrected_ns": 45000
  },
  "errors": {
    "total_errors": 3,
    "errors_by_type": {
      "invalid_ccsds_header": 2,
      "timestamp_non_monotonic": 1
    }
  }
}
```

---

### 2.11 Health Endpoints

| Endpoint | Purpose | Response |
|----------|---------|----------|
| GET /health/live | Liveness probe | 200 if process is alive |
| GET /health/ready | Readiness probe | 200 if accepting commands |
| GET /health/startup | Startup probe | 200 after initialization complete |

---

## 3. gRPC API

### 3.1 Service Definition (Protobuf)

```protobuf
syntax = "proto3";
package must.replay.v1;

service ReplayService {
  // Control RPCs
  rpc LoadFile(LoadFileRequest) returns (LoadFileResponse);
  rpc Start(StartRequest) returns (StartResponse);
  rpc Pause(PauseRequest) returns (PauseResponse);
  rpc Resume(ResumeRequest) returns (ResumeResponse);
  rpc Stop(StopRequest) returns (StopResponse);
  rpc Seek(SeekRequest) returns (SeekResponse);
  rpc SetSpeed(SetSpeedRequest) returns (SetSpeedResponse);
  rpc SetLoop(SetLoopRequest) returns (SetLoopResponse);
  
  // Query RPCs
  rpc GetStatus(GetStatusRequest) returns (GetStatusResponse);
  rpc GetStatistics(GetStatisticsRequest) returns (GetStatisticsResponse);
  
  // Streaming RPCs
  rpc StreamTelemetry(StreamTelemetryRequest) returns (stream TelemetryEnvelope);
  rpc StreamEvents(StreamEventsRequest) returns (stream ReplayEvent);
}
```

### 3.2 Key Messages

```protobuf
message TelemetryEnvelope {
  uint64 sequence_number = 1;
  uint64 original_timestamp_ns = 2;
  uint64 replay_timestamp_ns = 3;
  string source_id = 4;
  uint64 file_offset = 5;
  bytes payload = 6;
  uint32 payload_size = 7;
}

message ReplayEvent {
  string event_id = 1;
  EventType event_type = 2;
  uint64 timestamp_ns = 3;
  string message = 4;
  map<string, string> metadata = 5;
}

enum EventType {
  EVENT_TYPE_UNSPECIFIED = 0;
  PLAYBACK_STARTED = 1;
  PLAYBACK_PAUSED = 2;
  PLAYBACK_RESUMED = 3;
  PLAYBACK_FINISHED = 4;
  PLAYBACK_ERROR = 5;
  PACKET_PUBLISHED = 6;
  STATUS_CHANGED = 7;
}

enum PlaybackState {
  PLAYBACK_STATE_UNSPECIFIED = 0;
  IDLE = 1;
  READY = 2;
  RUNNING = 3;
  PAUSED = 4;
  STOPPED = 5;
  COMPLETED = 6;
  ERROR = 7;
}
```

### 3.3 Streaming Design

**StreamTelemetry:** Server-side streaming RPC. Client connects once, receives a continuous stream of `TelemetryEnvelope` messages as packets are replayed. The stream:
- Starts when playback enters RUNNING
- Pauses delivery when PAUSED (stream stays open, no messages sent)
- Resumes on RESUME
- Ends on STOP, COMPLETED, or ERROR
- Supports gRPC flow control for backpressure

**StreamEvents:** Server-side streaming RPC for operational events. Separate from telemetry to allow independent subscription.

**Why separate streams:** Telemetry is high-volume (100K+ pkt/s at 32x). Events are low-volume. Mixing them would require client-side demultiplexing and prevent independent backpressure.

---

## 4. Error Code Registry

| Code | HTTP | gRPC | Description |
|------|------|------|-------------|
| INVALID_REQUEST | 400 | INVALID_ARGUMENT | Malformed request |
| FILE_NOT_FOUND | 404 | NOT_FOUND | Specified file does not exist |
| INVALID_STATE | 409 | FAILED_PRECONDITION | Command not valid in current state |
| INVALID_FILE | 422 | INVALID_ARGUMENT | File validation failed |
| INTERNAL | 500 | INTERNAL | Unexpected internal error |
| UNAVAILABLE | 503 | UNAVAILABLE | Service not ready |

**Why unified error codes:** Both APIs must return the same error semantics. A command that fails via REST with 409 must fail via gRPC with FAILED_PRECONDITION for the same reason.

---

## 5. Revision History

| Version | Date       | Description    |
|---------|------------|----------------|
| 1.0.0   | 2026-07-03 | Initial draft  |

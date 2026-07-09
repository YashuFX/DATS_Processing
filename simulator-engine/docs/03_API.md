# MuST Replay Simulator Service — API Specification

| Field              | Value                                    |
|--------------------|------------------------------------------|
| **Document ID**    | MUST-SIM-API-003                         |
| **Version**        | 1.0.0-DRAFT                             |
| **Date**           | 2026-07-09                               |
| **Status**         | DRAFT — PENDING REVIEW                   |

---

## 1. API Design Philosophy

The Replay Simulator exposes two parallel API interfaces:
1. **REST API (Axum)**: For external operator control, diagnostics, and status monitoring.
2. **gRPC Control API (Tonic)**: For service-to-service automation and remote system management.

**Telemetry Output Ingress Client**: Unlike the control APIs which listen for commands, the Replay Simulator publishes replay packets by acting as a gRPC client to the Telemetry Gateway. It pushes a persistent stream of `TelemetryEnvelope` messages over the Gateway's `TelemetryIngressService`.

---

## 2. REST API Specification

Base URL: `http://{host}:{port}`

### 2.1 POST /api/v1/replay/load

**Purpose:** Load a telemetry file for replay.

*   **Method**: POST
*   **Path**: `/api/v1/replay/load`

**Request Body:**
```json
{
  "file_path": "data/sample.bin",
  "file_type": "binary",
  "target_stage": 0
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `file_path` | string | yes | Path to the playback file |
| `file_type` | string | yes | "binary" or "ccsds" |
| `target_stage` | integer | no | Target ProcessingStage (0 = RAW) |

**Response 200:**
```json
{
  "status": "READY",
  "file": {
    "path": "data/sample.bin",
    "size_bytes": 102400,
    "estimated_packets": 500,
    "estimated_duration_seconds": 10.0,
    "file_type": "binary"
  }
}
```

---

### 2.2 POST /api/v1/replay/start

**Purpose:** Begin playback from the current position.

*   **Method**: POST
*   **Path**: `/api/v1/replay/start`

**Request Body:**
```json
{
  "speed": 1.0,
  "loop_enabled": false
}
```

**Response 200:**
```json
{
  "status": "RUNNING",
  "speed": 1.0,
  "started_at": "2026-07-09T12:00:00Z"
}
```

---

### 2.3 POST /api/v1/replay/pause

**Purpose:** Pause playback, freezing the replay clock.

*   **Method**: POST
*   **Path**: `/api/v1/replay/pause`

**Response 200:**
```json
{
  "status": "PAUSED",
  "paused_at_packet": 125,
  "paused_at_timestamp": 1720000000000000000
}
```

---

### 2.4 POST /api/v1/replay/resume

**Purpose:** Resume playback from the paused position.

*   **Method**: POST
*   **Path**: `/api/v1/replay/resume`

**Response 200:**
```json
{
  "status": "RUNNING",
  "resumed_at_packet": 125
}
```

---

### 2.5 POST /api/v1/replay/stop

**Purpose:** Stop playback and reset to the beginning of the file.

*   **Method**: POST
*   **Path**: `/api/v1/replay/stop`

**Response 200:**
```json
{
  "status": "STOPPED"
}
```

---

### 2.6 POST /api/v1/replay/seek

**Purpose:** Jump to a specific timestamp within the loaded file.

*   **Method**: POST
*   **Path**: `/api/v1/replay/seek`

**Request Body:**
```json
{
  "target_timestamp": 1720000000500000000
}
```

**Response 200:**
```json
{
  "status": "PAUSED",
  "seeked_to_packet": 250,
  "seeked_to_timestamp": 1720000000500000000
}
```

---

### 2.7 POST /api/v1/replay/speed

**Purpose:** Change playback speed dynamically.

*   **Method**: POST
*   **Path**: `/api/v1/replay/speed`

**Request Body:**
```json
{
  "speed": 4.0
}
```

**Response 200:**
```json
{
  "speed": 4.0,
  "previous_speed": 1.0
}
```

---

### 2.8 POST /api/v1/replay/loop

**Purpose:** Enable or disable loop playback mode.

*   **Method**: POST
*   **Path**: `/api/v1/replay/loop`

**Request Body:**
```json
{
  "enabled": true
}
```

**Response 200:**
```json
{
  "loop_enabled": true
}
```

---

### 2.9 GET /api/v1/replay/status

**Purpose:** Get current playback state, configuration, and progress statistics.

*   **Method**: GET
*   **Path**: `/api/v1/replay/status`

**Response 200:**
```json
{
  "state": "RUNNING",
  "playback": {
    "speed": 4.0,
    "loop_enabled": false
  },
  "progress": {
    "packets_published": 125,
    "total_packets_estimated": 500,
    "progress_percent": 25.0,
    "current_timestamp": 1720000000225000000
  }
}
```

---

### 2.10 GET /api/v1/replay/packets

**Purpose:** Retrieve a list of recently transmitted telemetry packets.

*   **Method**: GET
*   **Path**: `/api/v1/replay/packets`

**Response 200:**
```json
[
  {
    "sequence_number": 125,
    "apid": 42,
    "timestamp_ns": 1720000000225000000,
    "payload_len": 64
  }
]
```

---

### 2.11 Health Endpoints

*   `GET /health/live`: Liveness check (returns "OK")
*   `GET /health/ready`: Readiness check (returns "OK")
*   `GET /health/startup`: Startup check (returns "OK")

---

## 3. gRPC Control API

### 3.1 Service Definition (`service.proto`)

```protobuf
syntax = "proto3";
package must.replay.v1;

import "must/telemetry/v1/envelope.proto";

service ReplayService {
  rpc LoadFile(LoadFileRequest) returns (LoadFileResponse);
  rpc StartPlayback(StartPlaybackRequest) returns (StartPlaybackResponse);
  rpc PausePlayback(PausePlaybackRequest) returns (PausePlaybackResponse);
  rpc ResumePlayback(ResumePlaybackRequest) returns (ResumePlaybackResponse);
  rpc SeekPlayback(SeekPlaybackRequest) returns (SeekPlaybackResponse);
  rpc StopPlayback(StopPlaybackRequest) returns (StopPlaybackResponse);
  rpc GetPlaybackStatus(GetPlaybackStatusRequest) returns (GetPlaybackStatusResponse);
}

message LoadFileRequest {
  string file_path = 1;
  string file_type = 2; // "binary" or "ccsds"
  must.telemetry.v1.ProcessingStage target_stage = 3;
}

message LoadFileResponse {
  bool success = 1;
  string message = 2;
  uint64 total_packets = 3;
  uint64 duration_ns = 4;
}

message StartPlaybackRequest {
  double speed = 1;
  bool loop = 2;
}

message StartPlaybackResponse {
  bool success = 1;
  string message = 2;
}

message PausePlaybackRequest {}

message PausePlaybackResponse {
  bool success = 1;
}

message ResumePlaybackRequest {}

message ResumePlaybackResponse {
  bool success = 1;
}

message SeekPlaybackRequest {
  uint64 target_timestamp_ns = 1;
}

message SeekPlaybackResponse {
  bool success = 1;
}

message StopPlaybackRequest {}

message StopPlaybackResponse {
  bool success = 1;
}

message GetPlaybackStatusRequest {}

message GetPlaybackStatusResponse {
  string state = 1; // "IDLE", "READY", "RUNNING", "PAUSED", "STOPPED", "COMPLETED", "ERROR"
  double speed = 2;
  double progress = 3; // 0.0 to 1.0
  uint64 packets_published = 4;
  uint64 current_timestamp_ns = 5;
}
```

---

## 4. Error Code Registry

| Replay Error Variant | HTTP Code | gRPC Status Code | Description |
|----------------------|-----------|------------------|-------------|
| `Configuration` | 400 | INVALID_ARGUMENT | Malformed request or config parameters |
| `FileIo` | 404 | NOT_FOUND | Playback file does not exist |
| `InvalidTransition` | 409 | FAILED_PRECONDITION | Command not valid in current player state |
| `PacketCorruption` | 422 | DATA_LOSS / INVALID_ARGUMENT | Packet validation or framing failed |
| `Network` | 503 | UNAVAILABLE | Downstream Telemetry Gateway unreachable |

---

## 5. Revision History

| Version | Date       | Description |
|---------|------------|-------------|
| 1.0.0   | 2026-07-09 | Synchronized with Version 1 Rust (Tonic/Axum) implementation |

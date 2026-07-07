/// # Publisher Contract
///
/// This module defines the exact contract between the Replay Simulator Service (RSS)
/// and the downstream Telemetry Gateway. It specifies what is published, how it is
/// published, and how backpressure is handled.
///
/// ## Transport Protocol
///
/// - **gRPC client-streaming** over HTTP/2 (defined in `must.gateway.v1.ingestion.proto`).
/// - The RSS opens a single persistent `StreamTelemetry` RPC call to the gateway.
/// - Each packet is sent as a `TelemetryStreamRequest` message within the stream.
/// - The stream remains open for the entire replay session; the gateway responds
///   with a single `TelemetryStreamResponse` when the stream closes.
///
/// ## Wire Message Structure
///
/// Each message on the wire is a `TelemetryStreamRequest`:
///
/// ```text
/// TelemetryStreamRequest {
///   source_id:  "rss-replay"              // Registered source ID at the gateway
///   session_id: "<uuid>"                  // Unique per replay session (generated at Start)
///   envelope:   TelemetryEnvelope { ... } // The full packet envelope
/// }
/// ```
///
/// ## TelemetryEnvelope Contents
///
/// The `TelemetryEnvelope` protobuf (defined in `must.telemetry.v1.envelope.proto`)
/// contains the complete packet context:
///
/// | Field                | Type                    | Description                                   |
/// |----------------------|-------------------------|-----------------------------------------------|
/// | `envelope_id`        | `string`                | UUID v4, unique per packet                    |
/// | `sequence_number`    | `uint64`                | Monotonic counter (1-based)                   |
/// | `source`             | `SourceIdentifier`      | `source_type=REPLAY`, `source_id="rss-replay"`|
/// | `station`            | `GroundStationIdentifier`| Optional ground station context              |
/// | `mission`            | `MissionIdentifier`     | Optional mission context                      |
/// | `satellite`          | `SatelliteIdentifier`   | Optional satellite context                    |
/// | `original_timestamp` | `MustTimestamp`          | Onboard/original timestamp (ONBOARD)          |
/// | `receive_timestamp`  | `MustTimestamp`          | Same as original for replay (GROUND)          |
/// | `publish_timestamp`  | `MustTimestamp`          | Wall-clock now() at publish time (REPLAY)     |
/// | `raw_packet`         | `RawTelemetryPacket`    | Raw bytes + length + offset                  |
/// | `ccsds_header`       | `CcsdsPacketHeader`     | Parsed primary header (CCSDS only)            |
/// | `ccsds_secondary`    | `CcsdsSecondaryHeader`  | Parsed secondary header (CCSDS only)          |
/// | `apid`               | `uint32`                | Application Process ID (0 for binary)         |
/// | `vcid`               | `uint32`                | Virtual Channel ID (0 default)                |
/// | `stage`              | `ProcessingStage`       | Configured target stage                       |
/// | `annotations`        | `map<string, string>`   | Extensible metadata (empty by default)        |
/// | `quality`            | `QualityIndicator`      | Validity flags (all true for replay)          |
///
/// ## Gateway Registration
///
/// Before streaming, the RSS must register itself with the gateway via REST:
///
/// ```text
/// POST /gateway/register-source
/// {
///   "source_type": "REPLAY",
///   "source_name": "replay-sim-01",
///   "mission":    { "mission_id": 1, "mission_name": "...", "mission_code": "..." },
///   "satellite":  { "satellite_id": ..., "satellite_name": "...", "norad_id": ... },
///   "station":    { "station_id": ..., "station_name": "...", "station_code": "..." }
/// }
/// → Returns { "source_id": "<uuid>", "status": "REGISTERED" }
/// ```
///
/// The returned `source_id` is used in every `TelemetryStreamRequest.source_id`.
///
/// ## Backpressure Protocol
///
/// 1. The RSS publishes via a bounded `mpsc` channel (default 1024 messages).
/// 2. When the channel reaches 90% capacity → `BackpressureStatus::HighWatermark`.
/// 3. When the channel is full → `try_send` fails → `ReplayError::Network`.
/// 4. The scheduler retries or pauses depending on configuration.
///
/// ## Routing at the Gateway
///
/// The Telemetry Gateway (per ADR-001 and ADR-007) publishes validated envelopes to
/// RabbitMQ with the following routing:
///
/// ```text
/// Exchange:    telemetry.raw  (topic exchange)
/// Routing Key: telemetry.{source_type}.{mission_code}.{apid}
/// Example:     telemetry.replay.CY3.42
/// ```
///
/// The RSS does NOT directly publish to RabbitMQ. It streams to the gateway, which
/// handles validation, enrichment, and bus publishing.

use crate::domain::replay_packet::ReplayPacket;
use crate::domain::envelope_builder::{EnvelopeBuilder, EnvelopeBuilderConfig};
use crate::domain::errors::ReplayError;
use crate::api::telemetry::v1::TelemetryEnvelope;
use crate::api::gateway::v1::TelemetryStreamRequest;

/// Represents the complete publication context for a single replay session.
/// This is the high-level contract object that the scheduler uses to publish packets.
pub struct PublishContract {
    /// The registered source_id from the gateway.
    pub source_id: String,
    /// Unique session identifier (UUID v4, generated at playback start).
    pub session_id: String,
    /// The envelope builder with session-level configuration.
    envelope_builder: EnvelopeBuilder,
}

impl PublishContract {
    /// Creates a new PublishContract for a replay session.
    pub fn new(source_id: String, session_id: String, config: EnvelopeBuilderConfig) -> Self {
        Self {
            source_id,
            session_id,
            envelope_builder: EnvelopeBuilder::new(config),
        }
    }

    /// Builds a complete `TelemetryEnvelope` from a `ReplayPacket`.
    pub fn build_envelope(&self, packet: &ReplayPacket) -> TelemetryEnvelope {
        self.envelope_builder.build(packet)
    }

    /// Builds the full wire-format `TelemetryStreamRequest` ready for gRPC transmission.
    pub fn build_request(&self, packet: &ReplayPacket) -> TelemetryStreamRequest {
        TelemetryStreamRequest {
            source_id: self.source_id.clone(),
            session_id: self.session_id.clone(),
            envelope: Some(self.build_envelope(packet)),
        }
    }

    /// Validates that a packet meets the minimum publication requirements.
    pub fn validate_packet(&self, packet: &ReplayPacket) -> Result<(), ReplayError> {
        if packet.payload.is_empty() {
            return Err(ReplayError::PacketCorruption(
                "Cannot publish packet with empty payload".to_string(),
            ));
        }
        if packet.original_timestamp_ns == 0 {
            return Err(ReplayError::TimestampCorruption(
                "Cannot publish packet with zero timestamp".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::telemetry::v1::ProcessingStage;

    fn make_test_packet() -> ReplayPacket {
        ReplayPacket {
            sequence_number: 1,
            original_timestamp_ns: 1_720_000_000_000_000_000,
            file_offset: 0,
            raw_data: vec![0x01, 0x02, 0x03],
            payload: vec![0x01, 0x02, 0x03],
            payload_length: 3,
            ccsds: None,
        }
    }

    #[test]
    fn test_build_request() {
        let config = EnvelopeBuilderConfig::default();
        let contract = PublishContract::new(
            "src-123".to_string(),
            "session-456".to_string(),
            config,
        );
        let pkt = make_test_packet();

        let req = contract.build_request(&pkt);
        assert_eq!(req.source_id, "src-123");
        assert_eq!(req.session_id, "session-456");
        assert!(req.envelope.is_some());

        let env = req.envelope.unwrap();
        assert_eq!(env.sequence_number, 1);
        assert_eq!(env.stage, ProcessingStage::Raw as i32);
    }

    #[test]
    fn test_validate_packet_empty_payload() {
        let config = EnvelopeBuilderConfig::default();
        let contract = PublishContract::new("s".into(), "s".into(), config);

        let mut pkt = make_test_packet();
        pkt.payload = Vec::new();

        let result = contract.validate_packet(&pkt);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_packet_zero_timestamp() {
        let config = EnvelopeBuilderConfig::default();
        let contract = PublishContract::new("s".into(), "s".into(), config);

        let mut pkt = make_test_packet();
        pkt.original_timestamp_ns = 0;

        let result = contract.validate_packet(&pkt);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_packet_valid() {
        let config = EnvelopeBuilderConfig::default();
        let contract = PublishContract::new("s".into(), "s".into(), config);
        let pkt = make_test_packet();

        assert!(contract.validate_packet(&pkt).is_ok());
    }
}

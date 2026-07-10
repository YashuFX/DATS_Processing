use std::time::{SystemTime, UNIX_EPOCH};
use crate::domain::replay_packet::ReplayPacket;
use crate::api::telemetry::v1::{
    TelemetryEnvelope, RawTelemetryPacket, QualityIndicator, ProcessingStage,
    CcsdsPacketHeader, CcsdsSecondaryHeader, PacketType, SequenceFlags, TimeCodeFormat,
};
use crate::api::common::v1::{
    MustTimestamp, TimestampSource, SourceIdentifier, SourceType,
    MissionIdentifier, SatelliteIdentifier, GroundStationIdentifier,
};

/// Configuration for the envelope builder — session-level metadata that stays
/// constant across all packets in a single replay session.
#[derive(Debug, Clone)]
pub struct EnvelopeBuilderConfig {
    /// Unique source identifier for this replay instance.
    pub source_id: String,
    /// Human-readable source name.
    pub source_name: String,
    /// The processing stage to stamp envelopes with (e.g., RAW, CCSDS_DECODED).
    pub target_stage: ProcessingStage,
    /// Optional mission context (populated from the LoadFile command or config).
    pub mission: Option<MissionIdentifier>,
    /// Optional satellite context.
    pub satellite: Option<SatelliteIdentifier>,
    /// Optional ground station context.
    pub station: Option<GroundStationIdentifier>,
}

impl Default for EnvelopeBuilderConfig {
    fn default() -> Self {
        Self {
            source_id: "rss-replay".to_string(),
            source_name: "Replay Simulator Service".to_string(),
            target_stage: ProcessingStage::Raw,
            mission: None,
            satellite: None,
            station: None,
        }
    }
}

/// Builds `TelemetryEnvelope` protobuf messages from `ReplayPacket` domain objects.
///
/// The builder is stateless per invocation — all session-level context is held in
/// `EnvelopeBuilderConfig`. This makes it trivially testable without needing
/// a running scheduler or network connection.
///
/// ## Envelope Field Mapping
///
/// | Envelope Field        | Source                                            |
/// |-----------------------|---------------------------------------------------|
/// | `envelope_id`         | UUID v4, generated per packet                     |
/// | `sequence_number`     | `ReplayPacket::sequence_number`                   |
/// | `source`              | Config `source_id` + `SOURCE_TYPE_REPLAY`         |
/// | `station`             | Config (optional)                                 |
/// | `mission`             | Config (optional)                                 |
/// | `satellite`           | Config (optional)                                 |
/// | `original_timestamp`  | `ReplayPacket::original_timestamp_ns` (ONBOARD)   |
/// | `receive_timestamp`   | Same as original (replayed, so same value, GROUND) |
/// | `publish_timestamp`   | Wall-clock `now()` at publish time (REPLAY)       |
/// | `raw_packet`          | `ReplayPacket::payload` + metadata                |
/// | `ccsds_header`        | Parsed from `ReplayPacket::ccsds` (if present)    |
/// | `ccsds_secondary`     | Parsed from `ReplayPacket::ccsds` (if present)    |
/// | `apid`                | `ReplayPacket::apid()`                            |
/// | `vcid`                | `ReplayPacket::vcid()`                            |
/// | `stage`               | Config `target_stage`                             |
/// | `quality`             | Default: all OK, no warnings                      |
pub struct EnvelopeBuilder {
    config: EnvelopeBuilderConfig,
}

impl EnvelopeBuilder {
    /// Creates a new EnvelopeBuilder with the given session configuration.
    pub fn new(config: EnvelopeBuilderConfig) -> Self {
        Self { config }
    }

    /// Wraps a `ReplayPacket` into a fully populated `TelemetryEnvelope`.
    pub fn build(&self, packet: &ReplayPacket) -> TelemetryEnvelope {
        let now_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let envelope_id = uuid::Uuid::new_v4().to_string();

        // Build CCSDS headers if parsed fields are available
        let (ccsds_header, ccsds_secondary) = self.build_ccsds_headers(packet);

        TelemetryEnvelope {
            envelope_id,
            sequence_number: packet.sequence_number,
            parameters: Vec::new(),

            // Source identity
            source: Some(SourceIdentifier {
                source_id: self.config.source_id.clone(),
                source_type: SourceType::Replay as i32,
                source_name: self.config.source_name.clone(),
            }),
            station: self.config.station.clone(),
            mission: self.config.mission.clone(),
            satellite: self.config.satellite.clone(),

            // Three-timestamp model
            original_timestamp: Some(MustTimestamp {
                nanos_since_epoch: packet.original_timestamp_ns,
                source: TimestampSource::Onboard as i32,
            }),
            receive_timestamp: Some(MustTimestamp {
                nanos_since_epoch: packet.original_timestamp_ns,
                source: TimestampSource::Ground as i32,
            }),
            publish_timestamp: Some(MustTimestamp {
                nanos_since_epoch: now_nanos,
                source: TimestampSource::Replay as i32,
            }),

            // Raw packet data
            raw_packet: Some(RawTelemetryPacket {
                data: packet.payload.clone(),
                data_length: packet.payload_length,
                receive_time: Some(MustTimestamp {
                    nanos_since_epoch: packet.original_timestamp_ns,
                    source: TimestampSource::Ground as i32,
                }),
                file_offset: packet.file_offset,
            }),

            // Parsed CCSDS fields
            ccsds_header,
            ccsds_secondary,
            apid: packet.apid() as u32,
            vcid: packet.vcid(),

            // Processing stage
            stage: self.config.target_stage as i32,

            // Annotations (empty by default, extensible)
            annotations: std::collections::HashMap::new(),

            // Quality — defaults to all-OK for replayed data
            quality: Some(QualityIndicator {
                is_valid: true,
                crc_ok: true,
                timestamp_monotonic: true,
                sequence_continuous: true,
                warnings: Vec::new(),
            }),
        }
    }

    /// Extracts CCSDS header protobuf messages from parsed fields.
    fn build_ccsds_headers(&self, packet: &ReplayPacket) -> (Option<CcsdsPacketHeader>, Option<CcsdsSecondaryHeader>) {
        match &packet.ccsds {
            Some(fields) => {
                let header = CcsdsPacketHeader {
                    version_number: fields.version as u32,
                    packet_type: if fields.packet_type == 0 {
                        PacketType::Tm as i32
                    } else {
                        PacketType::Tc as i32
                    },
                    secondary_header_flag: fields.secondary_header_flag,
                    apid: fields.apid as u32,
                    sequence_flags: match fields.sequence_flags {
                        0 => SequenceFlags::Continuation as i32,
                        1 => SequenceFlags::First as i32,
                        2 => SequenceFlags::Last as i32,
                        3 => SequenceFlags::Standalone as i32,
                        _ => SequenceFlags::Unspecified as i32,
                    },
                    sequence_count: fields.sequence_count as u32,
                    data_length: fields.data_length as u32,
                };

                let secondary = CcsdsSecondaryHeader {
                    coarse_time: packet.original_timestamp_ns / 1_000_000_000, // seconds
                    fine_time: (packet.original_timestamp_ns % 1_000_000_000) as u32,
                    format: TimeCodeFormat::EpochNs as i32,
                };

                (Some(header), Some(secondary))
            }
            None => (None, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::replay_packet::ParsedCcsdsFields;

    fn make_test_packet(is_ccsds: bool) -> ReplayPacket {
        let ccsds = if is_ccsds {
            Some(ParsedCcsdsFields {
                version: 0,
                packet_type: 0, // TM
                secondary_header_flag: true,
                apid: 42,
                sequence_flags: 3, // standalone
                sequence_count: 100,
                data_length: 20,
                vcid: 0,
            })
        } else {
            None
        };

        ReplayPacket {
            sequence_number: 7,
            original_timestamp_ns: 1_720_000_000_000_000_000,
            file_offset: 1024,
            raw_data: vec![0xDE, 0xAD, 0xBE, 0xEF],
            payload: vec![0xDE, 0xAD, 0xBE, 0xEF],
            payload_length: 4,
            ccsds,
        }
    }

    #[test]
    fn test_envelope_builder_binary_packet() {
        let config = EnvelopeBuilderConfig::default();
        let builder = EnvelopeBuilder::new(config);
        let pkt = make_test_packet(false);

        let envelope = builder.build(&pkt);

        assert_eq!(envelope.sequence_number, 7);
        assert!(!envelope.envelope_id.is_empty());
        assert_eq!(envelope.apid, 0);
        assert!(envelope.ccsds_header.is_none());
        assert!(envelope.ccsds_secondary.is_none());

        // Verify source
        let src = envelope.source.unwrap();
        assert_eq!(src.source_id, "rss-replay");
        assert_eq!(src.source_type, SourceType::Replay as i32);

        // Verify timestamps
        let orig_ts = envelope.original_timestamp.unwrap();
        assert_eq!(orig_ts.nanos_since_epoch, 1_720_000_000_000_000_000);
        assert_eq!(orig_ts.source, TimestampSource::Onboard as i32);

        let pub_ts = envelope.publish_timestamp.unwrap();
        assert_eq!(pub_ts.source, TimestampSource::Replay as i32);
        assert!(pub_ts.nanos_since_epoch > 0);

        // Verify raw packet
        let raw = envelope.raw_packet.unwrap();
        assert_eq!(raw.data, vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(raw.data_length, 4);
        assert_eq!(raw.file_offset, 1024);

        // Verify quality
        let quality = envelope.quality.unwrap();
        assert!(quality.is_valid);
        assert!(quality.crc_ok);
    }

    #[test]
    fn test_envelope_builder_ccsds_packet() {
        let config = EnvelopeBuilderConfig {
            source_id: "replay-01".to_string(),
            source_name: "Test Replay".to_string(),
            target_stage: ProcessingStage::CcsdsDecoded,
            mission: Some(MissionIdentifier {
                mission_id: 1,
                mission_name: "Chandrayaan-3".to_string(),
                mission_code: "CY3".to_string(),
            }),
            satellite: None,
            station: None,
        };
        let builder = EnvelopeBuilder::new(config);
        let pkt = make_test_packet(true);

        let envelope = builder.build(&pkt);

        assert_eq!(envelope.apid, 42);
        assert_eq!(envelope.stage, ProcessingStage::CcsdsDecoded as i32);

        // Verify CCSDS header was populated
        let ccsds_hdr = envelope.ccsds_header.unwrap();
        assert_eq!(ccsds_hdr.apid, 42);
        assert_eq!(ccsds_hdr.sequence_count, 100);
        assert_eq!(ccsds_hdr.packet_type, PacketType::Tm as i32);
        assert_eq!(ccsds_hdr.sequence_flags, SequenceFlags::Standalone as i32);
        assert!(ccsds_hdr.secondary_header_flag);

        // Verify CCSDS secondary header
        let sec_hdr = envelope.ccsds_secondary.unwrap();
        assert_eq!(sec_hdr.format, TimeCodeFormat::EpochNs as i32);
        assert!(sec_hdr.coarse_time > 0);

        // Verify mission was passed through
        let mission = envelope.mission.unwrap();
        assert_eq!(mission.mission_code, "CY3");
    }
}

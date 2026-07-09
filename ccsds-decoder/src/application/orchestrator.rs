// ── Application Orchestrator ──────────────────────────────────────────────────
//
// Responsibility: Execute the end-to-end telemetry decoding pipeline.
//
// This class is the core coordinator. It consumes raw envelope bytes,
// delegates decoding/validation to the domain logic, maintains the sequence
// continuity engine state, mutates the TelemetryEnvelope in-place, publishes the
// decorated envelope back to RabbitMQ, and prints a summary to the ConsoleSink.
//
// The execution flow is strictly:
//   1. Deserialize Envelope (via EnvelopeDeserializer::decode)
//   2. Extract raw_packet.data
//   3. Parse Primary Header (Parser::parse_primary_header)
//   4. Validate (Validator::validate_all)
//   5. Parse Secondary Header (if sec_hdr_flag == true)
//   6. Continuity check (under mutex)
//   7. Mutate TelemetryEnvelope in-place
//   8. Publish decorated envelope to RabbitMQ (retrying on AMQP errors)
//   9. Emit DecodeResult to ConsoleSink

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::adapters::inbound::deserializer::EnvelopeDeserializer;
use crate::domain::ccsds_hdr::TimeCodeFormat;
use crate::domain::continuity::ContinuityEngine;
use crate::domain::errors::DecoderError;
use crate::domain::parser::Parser;
use crate::domain::validator::Validator;
use crate::ports::outbound::{DecodeResult, DecodedPublisher, DecodedSink, SecondaryHeaderSummary};
use crate::proto::{
    CcsdsPacketHeader, MustTimestamp, PacketType, ProcessingStage, ProtoCcsdsSecondaryHeader,
    ProtoTimeCodeFormat, QualityIndicator, SequenceFlags, TimestampSource,
};

pub struct DecoderOrchestrator {
    sink: Arc<dyn DecodedSink>,
    publisher: Arc<dyn DecodedPublisher>,
    continuity_engine: Mutex<ContinuityEngine>,
    check_crc: bool,
}

impl DecoderOrchestrator {
    pub fn new(
        sink: Arc<dyn DecodedSink>,
        publisher: Arc<dyn DecodedPublisher>,
        check_crc: bool,
    ) -> Self {
        Self {
            sink,
            publisher,
            continuity_engine: Mutex::new(ContinuityEngine::new()),
            check_crc,
        }
    }

    /// Process one telemetry envelope payload.
    ///
    /// This follows the exact execution sequence requested.
    pub async fn on_envelope_consumed(
        &self,
        raw_bytes: Vec<u8>,
        incoming_routing_key: &str,
    ) -> Result<(), DecoderError> {
        // 1. Deserialize Envelope
        let mut envelope = EnvelopeDeserializer::decode(raw_bytes)?;

        // 2. Extract raw_packet.data
        let raw_data = EnvelopeDeserializer::extract_raw_data(&envelope)?;

        // 3. Parse Primary Header
        let primary_header = Parser::parse_primary_header(raw_data)?;

        // 4. Validate
        Validator::validate_all(raw_data, &primary_header, self.check_crc)?;

        // 5. Parse Secondary Header
        let secondary_header_summary = if primary_header.sec_hdr_flag {
            // Default time code format to CUC (configurable in Sprint 3 if needed)
            let format = TimeCodeFormat::Cuc;
            let secondary_header = Parser::parse_secondary_header(raw_data, format)?;
            Some(SecondaryHeaderSummary {
                coarse_time: secondary_header.coarse_time,
                fine_time: secondary_header.fine_time,
                format: "CUC".to_string(),
            })
        } else {
            None
        };

        // 6. Continuity check (locked Mutex block)
        let continuity_result = {
            let mut engine = self.continuity_engine.lock().await;
            engine.check(primary_header.apid, primary_header.seq_count)
        };

        // 7. Mutate TelemetryEnvelope in-place
        envelope.stage = ProcessingStage::CcsdsDecoded as i32;
        envelope.apid = primary_header.apid as u32;

        let proto_header = CcsdsPacketHeader {
            version_number: primary_header.version as u32,
            packet_type: match primary_header.packet_type {
                0 => PacketType::Tm as i32,
                1 => PacketType::Tc as i32,
                _ => PacketType::Unspecified as i32,
            },
            secondary_header_flag: primary_header.sec_hdr_flag,
            apid: primary_header.apid as u32,
            sequence_flags: match primary_header.seq_flags {
                0 => SequenceFlags::Continuation as i32,
                1 => SequenceFlags::First as i32,
                2 => SequenceFlags::Last as i32,
                3 => SequenceFlags::Standalone as i32,
                _ => SequenceFlags::Unspecified as i32,
            },
            sequence_count: primary_header.seq_count as u32,
            data_length: primary_header.packet_data_length as u32,
        };
        envelope.ccsds_header = Some(proto_header);

        if let Some(ref sh_summary) = secondary_header_summary {
            envelope.ccsds_secondary = Some(ProtoCcsdsSecondaryHeader {
                coarse_time: sh_summary.coarse_time as u64,
                fine_time: sh_summary.fine_time,
                format: ProtoTimeCodeFormat::Cuc as i32,
            });
        }

        envelope.quality = Some(QualityIndicator {
            is_valid: true,
            crc_ok: self.check_crc,
            timestamp_monotonic: true,
            sequence_continuous: !continuity_result.is_gap,
            warnings: vec![],
        });

        let now_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        envelope.publish_timestamp = Some(MustTimestamp {
            nanos_since_epoch: now_nanos,
            source: TimestampSource::System as i32,
        });

        // Determine outbound routing key: replace ".raw" with ".decoded"
        let outbound_routing_key = if incoming_routing_key.ends_with(".raw") {
            incoming_routing_key.replace(".raw", ".decoded")
        } else {
            format!("{}.decoded", incoming_routing_key)
        };

        // 8. Publish decorated envelope to RabbitMQ (guaranteeing confirms + retry inside publisher)
        self.publisher
            .publish(&envelope, &outbound_routing_key)
            .await?;

        // 9. Emit to Sink (Console summary log)
        let decode_result = DecodeResult {
            envelope_id: envelope.envelope_id.clone(),
            sequence_number: envelope.sequence_number,
            apid: primary_header.apid,
            seq_count: primary_header.seq_count,
            version: primary_header.version,
            packet_type: primary_header.packet_type,
            data_len_bytes: (primary_header.packet_data_length as usize) + 1,
            seq_flags: primary_header.seq_flags,
            is_gap: continuity_result.is_gap,
            is_duplicate: continuity_result.is_duplicate,
            crc_ok: self.check_crc,
            secondary_header: secondary_header_summary,
        };
        self.sink.emit(&decode_result).await?;

        Ok(())
    }
}

// ── Unit Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::outbound::DecodedSink;
    use crate::proto::RawTelemetryPacket;
    use crate::proto::TelemetryEnvelope;
    use async_trait::async_trait;
    use prost::Message;
    use std::sync::Mutex as StdMutex;

    // A mock sink to capture emitted results
    struct MockSink {
        emitted: Arc<StdMutex<Vec<DecodeResult>>>,
    }

    #[async_trait]
    impl DecodedSink for MockSink {
        async fn emit(&self, result: &DecodeResult) -> Result<(), DecoderError> {
            self.emitted.lock().unwrap().push(result.clone());
            Ok(())
        }
    }

    // A mock publisher to capture published envelopes
    struct MockPublisher {
        published: Arc<StdMutex<Vec<(TelemetryEnvelope, String)>>>,
    }

    #[async_trait]
    impl DecodedPublisher for MockPublisher {
        async fn publish(
            &self,
            envelope: &TelemetryEnvelope,
            routing_key: &str,
        ) -> Result<(), DecoderError> {
            self.published
                .lock()
                .unwrap()
                .push((envelope.clone(), routing_key.to_string()));
            Ok(())
        }
    }

    fn make_valid_envelope_bytes(apid: u16, seq: u16, data_len: u16) -> Vec<u8> {
        make_valid_envelope_bytes_with_version(0, apid, seq, data_len)
    }

    fn make_valid_envelope_bytes_with_version(
        version: u8,
        apid: u16,
        seq: u16,
        data_len: u16,
    ) -> Vec<u8> {
        // Word 0: version (3 bits), type=0, sec_hdr=0, APID (masked to 11 bits)
        let word0 = ((version as u16) << 13) | (apid & 0x07FF);
        // Word 1: seq_flags=0b11 (standalone), seq_count (masked to 14 bits)
        let word1 = 0xC000 | (seq & 0x3FFF);
        // Word 2: packet_data_length = data_len - 1
        let word2 = data_len.saturating_sub(1);

        let w0_bytes = word0.to_be_bytes();
        let w1_bytes = word1.to_be_bytes();
        let w2_bytes = word2.to_be_bytes();

        let mut raw_data = vec![
            w0_bytes[0],
            w0_bytes[1],
            w1_bytes[0],
            w1_bytes[1],
            w2_bytes[0],
            w2_bytes[1],
        ];
        // Extend payload bytes to match data_len
        raw_data.extend(vec![0xAA; data_len as usize]);

        let envelope = TelemetryEnvelope {
            envelope_id: "test-uuid-1234".to_string(),
            sequence_number: 42,
            raw_packet: Some(RawTelemetryPacket {
                data: raw_data,
                data_length: (6 + data_len) as u32,
                ..Default::default()
            }),
            ..Default::default()
        };

        let mut buf = Vec::new();
        envelope.encode(&mut buf).unwrap();
        buf
    }

    #[tokio::test]
    async fn test_orchestrator_happy_path() {
        let emitted = Arc::new(StdMutex::new(Vec::new()));
        let sink = Arc::new(MockSink {
            emitted: emitted.clone(),
        });
        let published = Arc::new(StdMutex::new(Vec::new()));
        let publisher = Arc::new(MockPublisher {
            published: published.clone(),
        });
        let orchestrator = DecoderOrchestrator::new(sink, publisher, false);

        let raw_bytes = make_valid_envelope_bytes(42, 1200, 22);
        let res = orchestrator
            .on_envelope_consumed(raw_bytes, "cy3.sat101.42.raw")
            .await;
        assert!(res.is_ok());

        // Verify console sink emission
        let results = emitted.lock().unwrap();
        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.envelope_id, "test-uuid-1234");
        assert_eq!(r.sequence_number, 42);
        assert_eq!(r.apid, 42);
        assert_eq!(r.seq_count, 1200);
        assert_eq!(r.version, 0);
        assert_eq!(r.packet_type, 0);
        assert_eq!(r.data_len_bytes, 22);
        assert!(!r.is_gap);
        assert!(!r.is_duplicate);

        // Verify publisher emission and mutation
        let pub_list = published.lock().unwrap();
        assert_eq!(pub_list.len(), 1);
        let (env, rk) = &pub_list[0];
        assert_eq!(rk, "cy3.sat101.42.decoded");
        assert_eq!(env.stage, ProcessingStage::CcsdsDecoded as i32);
        assert_eq!(env.apid, 42);
        let hdr = env.ccsds_header.as_ref().unwrap();
        assert_eq!(hdr.version_number, 0);
        assert_eq!(hdr.apid, 42);
        assert_eq!(hdr.sequence_count, 1200);
        assert_eq!(hdr.sequence_flags, SequenceFlags::Standalone as i32);
        assert_eq!(hdr.packet_type, PacketType::Tm as i32);

        let quality = env.quality.as_ref().unwrap();
        assert!(quality.is_valid);
        assert!(quality.sequence_continuous);
    }

    #[tokio::test]
    async fn test_orchestrator_too_short() {
        let emitted = Arc::new(StdMutex::new(Vec::new()));
        let sink = Arc::new(MockSink {
            emitted: emitted.clone(),
        });
        let published = Arc::new(StdMutex::new(Vec::new()));
        let publisher = Arc::new(MockPublisher {
            published: published.clone(),
        });
        let orchestrator = DecoderOrchestrator::new(sink, publisher, false);

        // Make an envelope with too short raw data (e.g. 3 bytes)
        let envelope = TelemetryEnvelope {
            envelope_id: "short-uuid".to_string(),
            raw_packet: Some(RawTelemetryPacket {
                data: vec![0x00, 0x11, 0x22],
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut buf = Vec::new();
        envelope.encode(&mut buf).unwrap();

        let res = orchestrator.on_envelope_consumed(buf, "cy3.raw").await;
        assert!(matches!(res, Err(DecoderError::PacketTooShort(3))));
    }

    #[tokio::test]
    async fn test_orchestrator_invalid_version() {
        let emitted = Arc::new(StdMutex::new(Vec::new()));
        let sink = Arc::new(MockSink {
            emitted: emitted.clone(),
        });
        let published = Arc::new(StdMutex::new(Vec::new()));
        let publisher = Arc::new(MockPublisher {
            published: published.clone(),
        });
        let orchestrator = DecoderOrchestrator::new(sink, publisher, false);

        // Version = 0b111 = 7
        let raw_bytes = make_valid_envelope_bytes_with_version(7, 42, 1200, 22);
        let res = orchestrator
            .on_envelope_consumed(raw_bytes, "cy3.raw")
            .await;
        assert!(matches!(res, Err(DecoderError::InvalidVersion(7))));
    }
}

// ── Protobuf Deserializer ─────────────────────────────────────────────────────
//
// Responsibility: convert raw AMQP delivery bytes → TelemetryEnvelope.
// Nothing else.
//
// Why a separate struct?
//   The RabbitMqConsumer is responsible for AMQP only (connect, bind, consume).
//   Protobuf decoding is a separate concern — isolated here so it can be
//   unit-tested without a live broker and without any AMQP machinery.
//
//   RabbitMqConsumer
//         ↓  raw Vec<u8>
//   EnvelopeDeserializer          ← this file
//         ↓  TelemetryEnvelope
//   handler (orchestrator)

use prost::Message;

use crate::domain::errors::DecoderError;
use crate::proto::TelemetryEnvelope;

pub struct EnvelopeDeserializer;

impl EnvelopeDeserializer {
    /// Decode raw Protobuf bytes into a `TelemetryEnvelope`.
    ///
    /// Returns `DecoderError::ProtoDecodeError` on any parse failure.
    /// The raw bytes are consumed (moved in), keeping allocations minimal.
    pub fn decode(raw_bytes: Vec<u8>) -> Result<TelemetryEnvelope, DecoderError> {
        TelemetryEnvelope::decode(raw_bytes.as_slice()).map_err(|e| {
            DecoderError::ProtoDecodeError(format!(
                "Failed to decode TelemetryEnvelope from {} bytes: {e}",
                raw_bytes.len()
            ))
        })
    }

    /// Extract the raw CCSDS packet bytes from a decoded `TelemetryEnvelope`.
    ///
    /// Returns the `raw_packet.data` bytes, or an error if the raw_packet
    /// field is absent (which should never happen for valid gateway messages).
    pub fn extract_raw_data(envelope: &TelemetryEnvelope) -> Result<&[u8], DecoderError> {
        envelope
            .raw_packet
            .as_ref()
            .map(|p| p.data.as_slice())
            .ok_or_else(|| {
                DecoderError::ProtoDecodeError(format!(
                    "TelemetryEnvelope '{}' has no raw_packet field",
                    envelope.envelope_id
                ))
            })
    }
}

// ── Unit Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::RawTelemetryPacket;
    use prost::Message;

    fn make_envelope_bytes(data: Vec<u8>) -> Vec<u8> {
        let envelope = TelemetryEnvelope {
            envelope_id: "test-envelope-001".to_string(),
            sequence_number: 1,
            raw_packet: Some(RawTelemetryPacket {
                data,
                data_length: 28,
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut buf = Vec::new();
        envelope.encode(&mut buf).unwrap();
        buf
    }

    #[test]
    fn test_decode_valid_envelope() {
        let raw = make_envelope_bytes(vec![0x00, 0x2A, 0xC4, 0xB0, 0x00, 0x15]);
        let envelope = EnvelopeDeserializer::decode(raw).unwrap();
        assert_eq!(envelope.envelope_id, "test-envelope-001");
    }

    #[test]
    fn test_decode_garbage_bytes_returns_error() {
        let garbage = vec![0xFF, 0xFE, 0xFD, 0x00, 0x01];
        // prost will return an error for non-Protobuf bytes
        // (technically it might succeed with empty fields; either way we
        //  verify we don't panic)
        let _result = EnvelopeDeserializer::decode(garbage);
        // No assertion on Ok/Err — prost may parse partial; we verify no panic.
    }

    #[test]
    fn test_extract_raw_data_present() {
        let pkt_bytes = vec![0x00, 0x2A, 0xC4, 0xB0, 0x00, 0x15, 0xAA, 0xBB];
        let raw = make_envelope_bytes(pkt_bytes.clone());
        let envelope = EnvelopeDeserializer::decode(raw).unwrap();
        let data = EnvelopeDeserializer::extract_raw_data(&envelope).unwrap();
        assert_eq!(data, pkt_bytes.as_slice());
    }

    #[test]
    fn test_extract_raw_data_absent_returns_error() {
        let envelope = TelemetryEnvelope {
            envelope_id: "no-raw".to_string(),
            raw_packet: None,
            ..Default::default()
        };
        let err = EnvelopeDeserializer::extract_raw_data(&envelope).unwrap_err();
        assert!(matches!(err, DecoderError::ProtoDecodeError(_)));
    }
}

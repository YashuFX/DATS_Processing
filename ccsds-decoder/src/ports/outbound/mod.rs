// ── Outbound Port: DecodedSink ────────────────────────────────────────────────
//
// This trait is the boundary between the application core and any downstream
// system (console, RabbitMQ publisher, database, etc.).
//
// In hexagonal architecture terms: this is the DRIVEN port — the adapter that
// the application core pushes data OUT to.
//
// Sprint 2 implementor: `adapters::outbound::console_sink::ConsoleSink`
// Sprint 3 implementor: `adapters::outbound::rabbitmq_publisher::RabbitMqPublisher`

use async_trait::async_trait;

use crate::domain::errors::DecoderError;

// ── DecodeResult ──────────────────────────────────────────────────────────────

/// The output contract of the `DecoderOrchestrator`.
///
/// This struct carries all fields produced by the full pipeline:
///   Parser → Validator → Continuity Engine
///
/// It is passed to `DecodedSink::emit()` so the sink can format or forward it.
#[derive(Debug, Clone)]
pub struct DecodeResult {
    // ── Identity ──────────────────────────────────────────────────────────
    /// The original `TelemetryEnvelope.envelope_id` — UUID string.
    pub envelope_id: String,

    /// The original `TelemetryEnvelope.sequence_number`.
    pub sequence_number: u64,

    // ── CCSDS Primary Header Fields ───────────────────────────────────────
    /// Application Process Identifier (11-bit, 0–2047).
    pub apid: u16,

    /// Packet Sequence Count (14-bit, 0–16383).
    pub seq_count: u16,

    /// CCSDS Version Number (always 0 for valid packets).
    pub version: u8,

    /// Packet Type — 0 = TM (telemetry), 1 = TC (telecommand).
    pub packet_type: u8,

    /// Declared data field length in bytes (packet_data_length + 1).
    pub data_len_bytes: usize,

    /// Sequence Flags — 0b11 = standalone, 0b01 = first, etc.
    pub seq_flags: u8,

    // ── Quality Fields ────────────────────────────────────────────────────
    /// True if `ContinuityEngine` detected a gap (packet(s) were dropped).
    pub is_gap: bool,

    /// True if `ContinuityEngine` detected a duplicate sequence count.
    pub is_duplicate: bool,

    /// True if CRC-16 validation passed (only meaningful when CHECK_CRC=true).
    pub crc_ok: bool,

    // ── Secondary Header (optional) ───────────────────────────────────────
    /// Present when `sec_hdr_flag == true` in the primary header.
    pub secondary_header: Option<SecondaryHeaderSummary>,
}

/// A summary of the decoded secondary header for emission.
#[derive(Debug, Clone)]
pub struct SecondaryHeaderSummary {
    pub coarse_time: u32,
    pub fine_time: u32,
    pub format: String, // "CUC", "CDS", "EPOCH_NS", "UNSPECIFIED"
}

// ── DecodedSink trait ─────────────────────────────────────────────────────────

/// An outbound port: a destination for processed decode results.
///
/// The orchestrator calls `emit()` after successfully completing the full
/// decode pipeline. It is the sink's responsibility to format, publish,
/// store, or log the result.
#[async_trait]
pub trait DecodedSink: Send + Sync {
    /// Emit the result of processing one decoded telemetry envelope.
    ///
    /// Returns `Ok(())` on success.
    /// Returns `Err(DecoderError::AmqpError(...))` if the downstream is
    /// unavailable (only applicable to the Sprint 3 publisher sink).
    async fn emit(&self, result: &DecodeResult) -> Result<(), DecoderError>;
}

// ── DecodedPublisher trait ──────────────────────────────────────────────────

#[async_trait]
pub trait DecodedPublisher: Send + Sync {
    /// Publishes the decorated telemetry envelope to the outbound exchange.
    async fn publish(
        &self,
        envelope: &crate::proto::TelemetryEnvelope,
        routing_key: &str,
    ) -> Result<(), DecoderError>;
}

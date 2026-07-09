// ── Domain Error Enumeration ─────────────────────────────────────────────────
//
// Every error that can occur inside the pure domain layer is represented here.
// Infrastructure errors (AMQP, IO) are mapped into these variants by adapters.

use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum DecoderError {
    /// Packet is too short to contain a valid CCSDS primary header (< 6 bytes).
    #[error("Packet too short: expected >= 6 bytes, got {0}")]
    PacketTooShort(usize),

    /// The CCSDS version field was non-zero.  CCSDS 133.0-B-2 mandates 0b000.
    #[error("Invalid CCSDS version: expected 0, got {0}")]
    InvalidVersion(u8),

    /// The declared data_length field does not match the actual byte count.
    #[error("Length mismatch: declared {declared}, actual {actual}")]
    LengthMismatch { declared: usize, actual: usize },

    /// CRC-16/CCITT validation failed.
    #[error("CRC mismatch: expected {expected:#06x}, computed {computed:#06x}")]
    CrcMismatch { expected: u16, computed: u16 },

    /// Secondary header parsing failed (offset overrun or unknown time code).
    #[error("Secondary header parse error: {0}")]
    SecondaryHeaderError(String),

    /// Catch-all for unexpected internal states.
    #[error("Internal decoder error: {0}")]
    Internal(String),

    // ── Infrastructure variants (Sprint 2) ────────────────────────────────
    // These are appended at the end — existing variant order is unchanged so
    // that no PartialEq-dependent test is broken.
    /// Protobuf deserialization of an incoming AMQP delivery failed.
    #[error("Protobuf decode error: {0}")]
    ProtoDecodeError(String),

    /// An AMQP connection or channel operation failed.
    #[error("AMQP error: {0}")]
    AmqpError(String),

    /// A required configuration value is missing or malformed.
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

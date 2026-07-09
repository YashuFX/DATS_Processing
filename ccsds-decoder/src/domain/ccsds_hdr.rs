// ── CCSDS Header Structs ─────────────────────────────────────────────────────
//
// Pure Rust representations of the CCSDS 133.0-B-2 Space Packet headers.
// These are internal domain types — they are later mapped to the Protobuf
// CcsdsPacketHeader / CcsdsSecondaryHeader types by the orchestrator.
//
// Why separate types?
//   The domain layer must NOT depend on generated Protobuf code.
//   Protobuf types belong to the infrastructure layer (api.rs).
//   Clean separation means the parser is testable without any Protobuf tooling.

/// Decoded CCSDS Space Packet Primary Header (6 bytes, CCSDS 133.0-B-2 §4.1)
#[derive(Debug, Clone, PartialEq)]
pub struct CcsdsPrimaryHeader {
    /// Packet Version Number — must always be 0b000 (= 0).
    pub version: u8,

    /// Packet Type — 0 = Telemetry (TM), 1 = Telecommand (TC).
    pub packet_type: u8,

    /// Secondary Header Flag — true if a secondary header is present.
    pub sec_hdr_flag: bool,

    /// Application Process Identifier — 11 bits, identifies the data source.
    pub apid: u16,

    /// Sequence Flags — 2 bits.
    ///   0b00 = Continuation, 0b01 = First, 0b10 = Last, 0b11 = Standalone.
    pub seq_flags: u8,

    /// Packet Sequence Count — 14 bits, wraps at 16383.
    pub seq_count: u16,

    /// Packet Data Length — number of bytes in the Packet Data Field minus 1.
    /// Actual data field size = packet_data_length + 1.
    pub packet_data_length: u16,
}

/// Decoded CCSDS Secondary Header (variable size, time-code dependent)
#[derive(Debug, Clone, PartialEq)]
pub struct CcsdsSecondaryHeader {
    /// Coarse time — seconds since epoch (GPS or TAI depending on mission).
    pub coarse_time: u32,

    /// Fine time — sub-second counter (resolution depends on time code format).
    pub fine_time: u32,

    /// Time code format identifier (maps to proto TimeCodeFormat enum).
    pub format: TimeCodeFormat,
}

/// Time code format detected or configured.
#[derive(Debug, Clone, PartialEq)]
pub enum TimeCodeFormat {
    /// CCSDS Unsegmented Time Code
    Cuc,
    /// CCSDS Day Segmented Time Code
    Cds,
    /// Generic nanoseconds since UNIX epoch
    EpochNs,
    /// Unknown / not present
    Unspecified,
}

/// Result of checking a packet's sequence continuity.
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceAnalysis {
    /// The expected next sequence count before this packet arrived.
    pub expected: u16,
    /// Whether this packet repeats an already-seen sequence count.
    pub is_duplicate: bool,
    /// Whether there is a gap between the expected and received sequence count.
    pub is_gap: bool,
}

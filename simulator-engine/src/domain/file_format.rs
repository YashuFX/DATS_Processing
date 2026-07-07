/// # Replay File Format Specification
///
/// The RSS supports two on-disk telemetry recording formats. Both are sequential,
/// append-only packet streams with no file-level header or trailer. The file IS
/// the packet stream; each packet is self-delineating.
///
/// ## Format 1: Binary (`.bin`)
///
/// Each packet frame is laid out as:
///
/// ```text
/// ┌────────────────┬────────────┬──────────────────────┬─────────────────────────┐
/// │ Sync Word      │ Payload Len│ Timestamp            │ Raw Payload             │
/// │ (4 bytes)      │ (2 bytes)  │ (8 bytes)            │ (variable)              │
/// ├────────────────┼────────────┼──────────────────────┼─────────────────────────┤
/// │ 0x1A 0x2B      │ Big-endian │ Big-endian u64       │ Arbitrary telemetry     │
/// │ 0x3C 0x4D      │ u16, size  │ nanoseconds since    │ bytes (CCSDS frame,     │
/// │ (magic marker) │ of payload │ Unix epoch           │ engineering data, etc.) │
/// │                │ only       │ (original onboard or │                         │
/// │                │            │ ground-receive time) │                         │
/// └────────────────┴────────────┴──────────────────────┴─────────────────────────┘
///
/// Total frame size = 4 + 2 + 8 + payload_len = 14 + payload_len bytes
/// ```
///
/// Field details:
/// - **Sync Word** (`BINARY_SYNC_WORD`): `[0x1A, 0x2B, 0x3C, 0x4D]`. Used for frame
///   synchronization and byte-level resync after corruption.
/// - **Payload Length**: Big-endian `u16`. Encodes the byte count of the Raw Payload
///   field only (excludes sync, length, and timestamp).
/// - **Timestamp**: Big-endian `u64`. Nanoseconds since Unix epoch (1970-01-01T00:00:00Z).
///   This is the *original* packet timestamp, either extracted from the onboard clock
///   or stamped by the ground station at receive time.
/// - **Raw Payload**: The actual telemetry bytes. For ISRO missions this is typically
///   a complete CCSDS Space Packet (primary + secondary header + user data).
///
/// ## Format 2: CCSDS (`.ccsds`)
///
/// Raw, concatenated CCSDS Space Packets with no framing envelope. Each packet is
/// self-delineating via the CCSDS primary header length field:
///
/// ```text
/// ┌───────────────────────────────────┬──────────────────────────┐
/// │ CCSDS Primary Header (6 bytes)   │ Data Field (variable)    │
/// ├────┬────┬────┬────┬────┬─────────┼──────────────────────────┤
/// │ B0 │ B1 │ B2 │ B3 │ B4 │ B5     │ Secondary Hdr + User Data│
/// └────┴────┴────┴────┴────┴─────────┴──────────────────────────┘
///
/// Primary Header fields (per CCSDS 133.0-B-2):
///   Bytes 0-1: Version(3b) | Type(1b) | SecHdrFlag(1b) | APID(11b)
///   Bytes 2-3: SeqFlags(2b) | SeqCount(14b)
///   Bytes 4-5: DataLength (total_packet_size - 7, big-endian u16)
///
/// Total packet size = DataLength + 7 bytes
/// ```
///
/// Timestamp extraction for CCSDS files:
/// - The secondary header flag (bit 3 of byte 0) MUST be set.
/// - The first 8 bytes of the data field (bytes 6-13) are interpreted as a
///   big-endian `u64` nanosecond timestamp.
///
/// ## Design Constraints
///
/// 1. **No file-level header**: Files are pure packet streams. Metadata (packet count,
///    duration, timestamp range) is computed at load time via eager indexing.
/// 2. **Monotonic timestamps assumed**: Packets should appear in chronological order.
///    Non-monotonic timestamps trigger a 1ms fallback delta in the timing engine.
/// 3. **Maximum packet size**: Configured via `replay.max_packet_size_bytes` (default
///    65542 bytes = max CCSDS packet). Packets exceeding this are rejected.
/// 4. **Byte alignment**: All multi-byte fields are big-endian (network byte order).

/// Magic sync word marking the start of each frame in the Binary format.
pub const BINARY_SYNC_WORD: [u8; 4] = [0x1A, 0x2B, 0x3C, 0x4D];

/// Length of the Binary format frame header (sync + length + timestamp).
pub const BINARY_FRAME_HEADER_LEN: usize = 14; // 4 sync + 2 len + 8 ts

/// Length of the CCSDS primary header.
pub const CCSDS_PRIMARY_HEADER_LEN: usize = 6;

/// Offset within the CCSDS data field where the 8-byte timestamp starts.
pub const CCSDS_TIMESTAMP_OFFSET: usize = 6; // immediately after primary header

/// Length of the timestamp field in both formats.
pub const TIMESTAMP_FIELD_LEN: usize = 8;

/// The two supported file format variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayFileFormat {
    /// Binary framed format with sync word + length + timestamp + payload.
    Binary,
    /// Raw concatenated CCSDS Space Packets.
    Ccsds,
}

impl ReplayFileFormat {
    /// Parses a format string into a variant.
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "binary" | "bin" => Ok(Self::Binary),
            "ccsds" => Ok(Self::Ccsds),
            other => Err(format!("Unsupported file format: '{}'", other)),
        }
    }

    /// Returns the canonical string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Binary => "binary",
            Self::Ccsds => "ccsds",
        }
    }
}

/// Describes the structure of a single parsed packet frame on disk.
/// This is returned during indexing to record where each packet lives.
#[derive(Debug, Clone)]
pub struct PacketFrameDescriptor {
    /// Byte offset of the frame start in the file.
    pub file_offset: u64,
    /// Total frame length on disk (header + payload).
    pub frame_length: u64,
    /// Extracted timestamp in nanoseconds since epoch.
    pub timestamp_ns: u64,
    /// Byte offset where the raw payload begins (relative to frame start).
    /// For Binary: 14 (after sync+len+ts). For CCSDS: 0 (entire packet is the payload).
    pub payload_offset: usize,
    /// Length of the raw payload bytes.
    pub payload_length: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_parsing() {
        assert_eq!(ReplayFileFormat::from_str("binary").unwrap(), ReplayFileFormat::Binary);
        assert_eq!(ReplayFileFormat::from_str("bin").unwrap(), ReplayFileFormat::Binary);
        assert_eq!(ReplayFileFormat::from_str("CCSDS").unwrap(), ReplayFileFormat::Ccsds);
        assert!(ReplayFileFormat::from_str("pcap").is_err());
    }

    #[test]
    fn test_format_round_trip() {
        assert_eq!(ReplayFileFormat::from_str(ReplayFileFormat::Binary.as_str()).unwrap(), ReplayFileFormat::Binary);
        assert_eq!(ReplayFileFormat::from_str(ReplayFileFormat::Ccsds.as_str()).unwrap(), ReplayFileFormat::Ccsds);
    }

    #[test]
    fn test_constants() {
        assert_eq!(BINARY_FRAME_HEADER_LEN, 14);
        assert_eq!(CCSDS_PRIMARY_HEADER_LEN, 6);
        assert_eq!(BINARY_SYNC_WORD, [0x1A, 0x2B, 0x3C, 0x4D]);
    }
}

use serde::{Serialize, Deserialize};

/// A fully parsed replay packet — the concrete domain model that bridges
/// raw on-disk bytes and the protobuf TelemetryEnvelope.
/// `ReplayPacket` contains all extracted metadata needed to build a complete envelope
/// without re-parsing the binary data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayPacket {
    // ── Identity ──────────────────────────────────────────────────────

    /// Monotonically increasing sequence number assigned during replay.
    pub sequence_number: u64,

    // ── Timing ────────────────────────────────────────────────────────

    /// Original timestamp extracted from the packet (nanoseconds since Unix epoch).
    /// For Binary format: read from the 8-byte timestamp field.
    /// For CCSDS format: read from the secondary header.
    pub original_timestamp_ns: u64,

    // ── Source Location ───────────────────────────────────────────────

    /// Byte offset where this packet's frame starts in the source file.
    pub file_offset: u64,

    // ── Raw Data ──────────────────────────────────────────────────────

    /// The complete raw bytes of the packet (including all headers).
    /// For Binary format: sync_word(4) + length(2) + timestamp(8) + payload(N).
    /// For CCSDS format: primary_header(6) + data_field(N).
    pub raw_data: Vec<u8>,

    /// The raw telemetry payload bytes only (headers stripped).
    /// For Binary format: the payload after sync+len+ts.
    /// For CCSDS format: the complete CCSDS packet (since the packet IS the payload).
    pub payload: Vec<u8>,

    /// Length of the payload in bytes.
    pub payload_length: u32,

    // ── Parsed CCSDS Fields (populated only for CCSDS format) ─────────

    /// Parsed CCSDS header fields. `None` for Binary format packets.
    pub ccsds: Option<ParsedCcsdsFields>,
}

/// Parsed CCSDS primary + secondary header fields extracted during packet reading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedCcsdsFields {
    /// Packet Version Number (3 bits, always 0 for Space Packets).
    pub version: u8,
    /// Packet Type: 0 = TM (telemetry), 1 = TC (telecommand).
    pub packet_type: u8,
    /// Whether the secondary header is present.
    pub secondary_header_flag: bool,
    /// Application Process Identifier (11 bits).
    pub apid: u16,
    /// Sequence Flags: 00=continuation, 01=first, 10=last, 11=standalone.
    pub sequence_flags: u8,
    /// Packet Sequence Count (14 bits, wrapping counter 0-16383).
    pub sequence_count: u16,
    /// Data Length field from the primary header (total_packet_length - 7).
    pub data_length: u16,
    /// Virtual Channel ID (optional metadata injected from Transfer Frame context when available, default 0).
    pub vcid: u32,
}

impl ReplayPacket {
    /// Creates a ReplayPacket from a raw Binary-format frame.
    ///
    /// Binary frame layout: sync(4) + length(2) + timestamp(8) + payload(N)
    pub fn from_binary_frame(
        raw_data: Vec<u8>,
        timestamp_ns: u64,
        file_offset: u64,
        sequence_number: u64,
    ) -> Self {
        let payload = if raw_data.len() > 14 {
            raw_data[14..].to_vec()
        } else {
            Vec::new()
        };
        let payload_length = payload.len() as u32;

        let ccsds = if payload.len() >= 6 {
            let version = (payload[0] >> 5) & 0x07;
            let packet_type = (payload[0] >> 4) & 0x01;
            let secondary_header_flag = (payload[0] & 0x08) != 0;
            let apid = (((payload[0] as u16) & 0x07) << 8) | (payload[1] as u16);
            let sequence_flags = (payload[2] >> 6) & 0x03;
            let sequence_count = (((payload[2] as u16) & 0x3F) << 8) | (payload[3] as u16);
            let data_length = u16::from_be_bytes([payload[4], payload[5]]);

            Some(ParsedCcsdsFields {
                version,
                packet_type,
                secondary_header_flag,
                apid,
                sequence_flags,
                sequence_count,
                data_length,
                vcid: 0, // VCID is optional metadata injected from Transfer Frame context when available, default 0
            })
        } else {
            None
        };

        Self {
            sequence_number,
            original_timestamp_ns: timestamp_ns,
            file_offset,
            raw_data,
            payload,
            payload_length,
            ccsds,
        }
    }

    /// Creates a ReplayPacket from a raw CCSDS Space Packet.
    ///
    /// Parses the 6-byte primary header and extracts APID, sequence count, etc.
    pub fn from_ccsds_frame(
        raw_data: Vec<u8>,
        timestamp_ns: u64,
        file_offset: u64,
        sequence_number: u64,
    ) -> Self {
        let ccsds = if raw_data.len() >= 6 {
            let version = (raw_data[0] >> 5) & 0x07;
            let packet_type = (raw_data[0] >> 4) & 0x01;
            let secondary_header_flag = (raw_data[0] & 0x08) != 0;
            let apid = (((raw_data[0] as u16) & 0x07) << 8) | (raw_data[1] as u16);
            let sequence_flags = (raw_data[2] >> 6) & 0x03;
            let sequence_count = (((raw_data[2] as u16) & 0x3F) << 8) | (raw_data[3] as u16);
            let data_length = u16::from_be_bytes([raw_data[4], raw_data[5]]);

            Some(ParsedCcsdsFields {
                version,
                packet_type,
                secondary_header_flag,
                apid,
                sequence_flags,
                sequence_count,
                data_length,
                vcid: 0, // VCID is optional metadata injected from Transfer Frame context when available, default 0
            })
        } else {
            None
        };

        // For CCSDS, the entire packet is the payload (no framing envelope to strip)
        let payload = raw_data.clone();
        let payload_length = payload.len() as u32;

        Self {
            sequence_number,
            original_timestamp_ns: timestamp_ns,
            file_offset,
            raw_data,
            payload,
            payload_length,
            ccsds,
        }
    }

    /// Returns the APID if this is a CCSDS packet, or 0 otherwise.
    pub fn apid(&self) -> u16 {
        self.ccsds.as_ref().map(|c| c.apid).unwrap_or(0)
    }

    /// Returns the VCID (optional metadata injected from Transfer Frame context when available, or 0).
    pub fn vcid(&self) -> u32 {
        self.ccsds.as_ref().map(|c| c.vcid).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_binary_frame(payload: &[u8], timestamp_ns: u64) -> Vec<u8> {
        let mut frame = Vec::new();
        // Sync word
        frame.extend_from_slice(&[0x1A, 0x2B, 0x3C, 0x4D]);
        // Payload length (2 bytes, big-endian)
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        // Timestamp (8 bytes, big-endian)
        frame.extend_from_slice(&timestamp_ns.to_be_bytes());
        // Payload
        frame.extend_from_slice(payload);
        frame
    }

    fn make_ccsds_packet(apid: u16, seq_count: u16, payload: &[u8]) -> Vec<u8> {
        let mut packet = Vec::new();
        // Byte 0: version(000) | type(0) | sec_hdr(1) | apid_high(3 bits)
        let b0 = 0x08 | ((apid >> 8) & 0x07) as u8;
        // Byte 1: apid_low (8 bits)
        let b1 = (apid & 0xFF) as u8;
        // Byte 2: seq_flags(11) | seq_count_high (6 bits)
        let b2 = 0xC0 | ((seq_count >> 8) & 0x3F) as u8;
        // Byte 3: seq_count_low (8 bits)
        let b3 = (seq_count & 0xFF) as u8;
        // Data length = 8 (timestamp) + payload_len - 1
        let data_len = (8 + payload.len() - 1) as u16;
        let [b4, b5] = data_len.to_be_bytes();

        packet.extend_from_slice(&[b0, b1, b2, b3, b4, b5]);
        // 8-byte timestamp as secondary header
        packet.extend_from_slice(&1000000000u64.to_be_bytes());
        // User data
        packet.extend_from_slice(payload);
        packet
    }

    #[test]
    fn test_from_binary_frame() {
        let payload = b"HELLO";
        let ts = 1720000000_000000000u64;
        let frame = make_binary_frame(payload, ts);

        let pkt = ReplayPacket::from_binary_frame(frame.clone(), ts, 0, 1);

        assert_eq!(pkt.sequence_number, 1);
        assert_eq!(pkt.original_timestamp_ns, ts);
        assert_eq!(pkt.file_offset, 0);
        assert_eq!(pkt.payload, b"HELLO");
        assert_eq!(pkt.payload_length, 5);
        assert!(pkt.ccsds.is_none());
        assert_eq!(pkt.apid(), 0);
    }

    #[test]
    fn test_from_ccsds_frame() {
        let user_data = b"TM_DATA";
        let packet = make_ccsds_packet(42, 100, user_data);
        let ts = 1000000000u64;

        let pkt = ReplayPacket::from_ccsds_frame(packet.clone(), ts, 512, 7);

        assert_eq!(pkt.sequence_number, 7);
        assert_eq!(pkt.original_timestamp_ns, ts);
        assert_eq!(pkt.file_offset, 512);
        assert_eq!(pkt.apid(), 42);
        assert!(pkt.ccsds.is_some());

        let ccsds = pkt.ccsds.unwrap();
        assert_eq!(ccsds.apid, 42);
        assert_eq!(ccsds.sequence_count, 100);
        assert!(ccsds.secondary_header_flag);
        assert_eq!(ccsds.sequence_flags, 3); // standalone
    }
}

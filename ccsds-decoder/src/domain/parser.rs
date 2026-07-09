// ── CCSDS Space Packet Parser ────────────────────────────────────────────────
//
// RESPONSIBILITY: Pure bit-extraction only.
//
// This module reads bytes and unpacks fields according to CCSDS 133.0-B-2.
// It does NOT make any judgement about whether the values are correct.
//
// Rule checking (version == 0, length match, CRC integrity) is the sole
// responsibility of `validator.rs`. This separation means:
//   • Parser tests verify bit-math, not business rules.
//   • Validator tests verify rules, not byte-layout.
//   • New packet formats can be added to Parser without touching validation.
//
// CCSDS 133.0-B-2 primary header memory layout (6 bytes total):
//
//  Byte 0          Byte 1          Byte 2          Byte 3
//  ┌───┬─┬─┬──────────────┐ ┌──┬──────────────────────────┐
//  │VER│T│S│     APID     │ │SF│     Packet Seq Count     │
//  │3b │1│1│     11b      │ │2b│          14b             │
//  └───┴─┴─┴──────────────┘ └──┴──────────────────────────┘
//  Byte 4          Byte 5
//  ┌────────────────────────┐
//  │     Packet Data Length │
//  │           16b          │
//  └────────────────────────┘
//
// Fields are packed in big-endian (network) byte order.

use crate::domain::ccsds_hdr::{CcsdsPrimaryHeader, CcsdsSecondaryHeader, TimeCodeFormat};
use crate::domain::errors::DecoderError;

pub struct Parser;

impl Parser {
    // ── Primary Header ───────────────────────────────────────────────────────

    /// Extract the 6-byte CCSDS primary header from a raw packet buffer.
    ///
    /// This function is a **pure bit-extractor**. It reads whatever values
    /// are in the bytes without asserting any rules. If the version field is
    /// 7 (0b111), it will be faithfully returned as 7.
    ///
    /// Structural rule-checking (version == 0, length consistency, CRC)
    /// is performed afterwards by `Validator`.
    ///
    /// Returns `DecoderError::PacketTooShort` if the slice is < 6 bytes.
    pub fn parse_primary_header(data: &[u8]) -> Result<CcsdsPrimaryHeader, DecoderError> {
        if data.len() < 6 {
            return Err(DecoderError::PacketTooShort(data.len()));
        }

        // ── Bytes 0-1: Packet ID word ────────────────────────────────────────
        //   Bit 15-13 : Version Number (3 bits)
        //   Bit 12    : Packet Type    (1 bit)
        //   Bit 11    : Sec Hdr Flag   (1 bit)
        //   Bit 10-0  : APID           (11 bits)
        let word0 = u16::from_be_bytes([data[0], data[1]]);

        let version = ((word0 >> 13) & 0x07) as u8;
        let packet_type = ((word0 >> 12) & 0x01) as u8;
        let sec_hdr_flag = ((word0 >> 11) & 0x01) != 0;
        let apid = word0 & 0x07FF;

        // ── Bytes 2-3: Packet Sequence Control word ──────────────────────────
        //   Bit 15-14 : Sequence Flags  (2 bits)
        //   Bit 13-0  : Sequence Count  (14 bits)
        let word1 = u16::from_be_bytes([data[2], data[3]]);

        let seq_flags = ((word1 >> 14) & 0x03) as u8;
        let seq_count = word1 & 0x3FFF;

        // ── Bytes 4-5: Packet Data Length ────────────────────────────────────
        //   16 bits — actual data field is (packet_data_length + 1) bytes.
        let packet_data_length = u16::from_be_bytes([data[4], data[5]]);

        Ok(CcsdsPrimaryHeader {
            version,
            packet_type,
            sec_hdr_flag,
            apid,
            seq_flags,
            seq_count,
            packet_data_length,
        })
    }

    // ── Secondary Header ─────────────────────────────────────────────────────

    /// Attempt to parse a minimal CCSDS time code from the first bytes of the
    /// packet data field (immediately after the 6-byte primary header).
    ///
    /// Sprint 1 supports the two most common formats:
    ///   • CUC: 4-byte coarse + 2-byte fine (total 6 bytes, CCSDS 301.0-B-4)
    ///   • CDS: 2-byte day + 4-byte ms-of-day (total 6 bytes, CCSDS 301.0-B-4)
    ///
    /// Returns `DecoderError::SecondaryHeaderError` if there are fewer than
    /// 6 bytes in the data field.
    pub fn parse_secondary_header(
        data: &[u8],
        format: TimeCodeFormat,
    ) -> Result<CcsdsSecondaryHeader, DecoderError> {
        // Data field starts at byte 6 (after the primary header).
        let data_field = data.get(6..).unwrap_or(&[]);

        if data_field.len() < 6 {
            return Err(DecoderError::SecondaryHeaderError(format!(
                "data field too short for secondary header: {} bytes",
                data_field.len()
            )));
        }

        let (coarse_time, fine_time, actual_format) = match format {
            TimeCodeFormat::Cuc => {
                // CUC: bytes 0-3 = coarse seconds, bytes 4-5 = fine (sub-second units)
                let coarse = u32::from_be_bytes([
                    data_field[0],
                    data_field[1],
                    data_field[2],
                    data_field[3],
                ]);
                let fine = u32::from_be_bytes([0, 0, data_field[4], data_field[5]]);
                (coarse, fine, TimeCodeFormat::Cuc)
            }
            TimeCodeFormat::Cds => {
                // CDS: bytes 0-1 = day count, bytes 2-5 = milliseconds of day
                let day = u16::from_be_bytes([data_field[0], data_field[1]]) as u32;
                let ms = u32::from_be_bytes([
                    data_field[2],
                    data_field[3],
                    data_field[4],
                    data_field[5],
                ]);
                // Coarse = seconds (day * 86400 + ms / 1000), fine = ms remainder
                let coarse = day * 86_400 + ms / 1_000;
                let fine = ms % 1_000;
                (coarse, fine, TimeCodeFormat::Cds)
            }
            _ => {
                // For EpochNs and Unspecified, read 4+2 bytes with no semantic meaning.
                let coarse = u32::from_be_bytes([
                    data_field[0],
                    data_field[1],
                    data_field[2],
                    data_field[3],
                ]);
                let fine = u32::from_be_bytes([0, 0, data_field[4], data_field[5]]);
                (coarse, fine, TimeCodeFormat::Unspecified)
            }
        };

        Ok(CcsdsSecondaryHeader {
            coarse_time,
            fine_time,
            format: actual_format,
        })
    }

    // ── CRC-16/CCITT (pure math) ─────────────────────────────────────────────

    /// Compute a CRC-16/CCITT (poly = 0x1021, init = 0xFFFF) over the given
    /// byte slice.
    ///
    /// This is a **pure computation** — no rules, no errors.
    /// The `Validator` calls this and then decides if the result matches the
    /// stored CRC appended to the packet.
    pub fn compute_crc16(data: &[u8]) -> u16 {
        let mut crc: u16 = 0xFFFF;
        for byte in data {
            crc ^= (*byte as u16) << 8;
            for _ in 0..8 {
                if crc & 0x8000 != 0 {
                    crc = (crc << 1) ^ 0x1021;
                } else {
                    crc <<= 1;
                }
            }
        }
        crc
    }
}

// ── Unit Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Build a minimal valid 6-byte-only CCSDS primary header byte sequence.
    ///
    /// APID=42, seq_count=1200, packet_data_length=21 (data field = 22 bytes).
    fn minimal_packet() -> Vec<u8> {
        // Word 0: version=0, type=0, sec_hdr=0, APID=42 (0x002A)
        //   => 0b000_0_0_00000101010 = 0x002A
        // Word 1: seq_flags=0b11 (standalone), seq_count=1200 (0x04B0)
        //   => 0b11_00_0100_1011_0000 = 0xC4B0
        // Word 2: packet_data_length = 21 (0x0015)
        let mut pkt = vec![
            0x00, 0x2A, // word0: version=0, type=TM, no sec-hdr, APID=42
            0xC4, 0xB0, // word1: seq_flags=standalone, seq_count=1200
            0x00, 0x15, // word2: data_length=21  → data field = 22 bytes
        ];
        // Pad 22 bytes of user data so the length check passes
        pkt.extend_from_slice(&[0xAA; 22]);
        pkt
    }

    // ── Primary Header Tests ──────────────────────────────────────────────────

    #[test]
    fn test_parse_primary_header_apid() {
        let pkt = minimal_packet();
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        assert_eq!(hdr.apid, 42);
    }

    #[test]
    fn test_parse_primary_header_version() {
        let pkt = minimal_packet();
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        assert_eq!(hdr.version, 0);
    }

    #[test]
    fn test_parse_primary_header_seq_count() {
        let pkt = minimal_packet();
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        assert_eq!(hdr.seq_count, 1200);
    }

    #[test]
    fn test_parse_primary_header_seq_flags_standalone() {
        let pkt = minimal_packet();
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        assert_eq!(hdr.seq_flags, 0b11); // standalone
    }

    #[test]
    fn test_parse_primary_header_no_secondary() {
        let pkt = minimal_packet();
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        assert!(!hdr.sec_hdr_flag);
    }

    #[test]
    fn test_parse_primary_header_packet_type_tm() {
        let pkt = minimal_packet();
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        assert_eq!(hdr.packet_type, 0); // TM
    }

    #[test]
    fn test_parse_primary_header_packet_type_tc() {
        // Flip bit 12 of byte 0 to set type=1 (TC)
        let mut pkt = minimal_packet();
        pkt[0] |= 0x10; // set bit 4 of byte 0 (bit 12 of word0)
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        assert_eq!(hdr.packet_type, 1); // TC
    }

    #[test]
    fn test_parse_primary_header_apid_max() {
        // APID = 2047 (0x7FF) — maximum possible value
        let mut pkt = minimal_packet();
        pkt[0] = (pkt[0] & 0xF8) | 0x07; // high 3 bits of APID
        pkt[1] = 0xFF; // low 8 bits of APID
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        assert_eq!(hdr.apid, 2047);
    }

    #[test]
    fn test_parse_primary_header_apid_zero() {
        // APID = 0 — minimum
        let mut pkt = minimal_packet();
        pkt[0] &= 0xF8; // clear high bits of APID
        pkt[1] = 0x00;
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        assert_eq!(hdr.apid, 0);
    }

    #[test]
    fn test_parse_primary_header_too_short() {
        let pkt = vec![0x00, 0x2A, 0xC4]; // only 3 bytes
        let err = Parser::parse_primary_header(&pkt).unwrap_err();
        assert_eq!(err, DecoderError::PacketTooShort(3));
    }

    #[test]
    fn test_parse_primary_header_exactly_6_bytes() {
        // Just 6 bytes — data field size = 0, but header itself is valid
        let pkt = vec![0x00, 0x2A, 0xC4, 0xB0, 0x00, 0x00];
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        assert_eq!(hdr.apid, 42);
    }

    #[test]
    fn test_parse_primary_header_invalid_version_is_extracted_faithfully() {
        // Parser does NOT reject invalid versions — it reads whatever bits are there.
        // The Validator is responsible for rejecting version != 0.
        let mut pkt = minimal_packet();
        pkt[0] |= 0xE0; // set version bits to 0b111 (= 7)
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        assert_eq!(hdr.version, 7); // faithfully extracted, not rejected
    }

    #[test]
    fn test_parse_primary_header_data_length_field() {
        let pkt = minimal_packet();
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        assert_eq!(hdr.packet_data_length, 21);
    }

    // ── Secondary Header Tests ────────────────────────────────────────────────

    fn packet_with_secondary_cuc() -> Vec<u8> {
        // Primary header with sec_hdr_flag set (bit 11 of word0)
        // 0x08_2A → sec_hdr_flag=1, APID=42
        let mut pkt = vec![
            0x08, 0x2A, // word0: sec_hdr_flag=1, APID=42
            0xC4, 0xB0, // word1
            0x00, 0x1D, // data_length=29 → data field=30 bytes
        ];
        // CUC secondary header: coarse=1000 (0x000003E8), fine=500 (0x01F4)
        pkt.extend_from_slice(&[0x00, 0x00, 0x03, 0xE8]); // coarse = 1000
        pkt.extend_from_slice(&[0x01, 0xF4]); // fine   = 500
                                              // Pad remainder with user data to reach declared 30-byte data field
        pkt.extend_from_slice(&[0xBB; 24]);
        pkt
    }

    #[test]
    fn test_parse_secondary_header_cuc_coarse() {
        let pkt = packet_with_secondary_cuc();
        let sec = Parser::parse_secondary_header(&pkt, TimeCodeFormat::Cuc).unwrap();
        assert_eq!(sec.coarse_time, 1000);
    }

    #[test]
    fn test_parse_secondary_header_cuc_fine() {
        let pkt = packet_with_secondary_cuc();
        let sec = Parser::parse_secondary_header(&pkt, TimeCodeFormat::Cuc).unwrap();
        assert_eq!(sec.fine_time, 500);
    }

    #[test]
    fn test_parse_secondary_header_cuc_format() {
        let pkt = packet_with_secondary_cuc();
        let sec = Parser::parse_secondary_header(&pkt, TimeCodeFormat::Cuc).unwrap();
        assert_eq!(sec.format, TimeCodeFormat::Cuc);
    }

    #[test]
    fn test_parse_secondary_header_cds_coarse() {
        let mut pkt = vec![0x08, 0x2A, 0xC4, 0xB0, 0x00, 0x1D];
        // CDS: day=1 (0x0001), ms_of_day=3_600_000 (= 1 hour = 0x0036EE80)
        pkt.extend_from_slice(&[0x00, 0x01]); // day=1
        pkt.extend_from_slice(&[0x00, 0x36, 0xEE, 0x80]); // ms_of_day = 3_600_000
        pkt.extend_from_slice(&[0xCC; 24]);
        let sec = Parser::parse_secondary_header(&pkt, TimeCodeFormat::Cds).unwrap();
        // Expected coarse = 1*86400 + 3_600_000/1_000 = 86400 + 3600 = 90000
        assert_eq!(sec.coarse_time, 90_000);
    }

    #[test]
    fn test_parse_secondary_header_too_short() {
        let pkt = vec![0x08, 0x2A, 0xC4, 0xB0, 0x00, 0x04, 0xAA, 0xBB]; // only 2 data bytes
        let err = Parser::parse_secondary_header(&pkt, TimeCodeFormat::Cuc).unwrap_err();
        assert!(matches!(err, DecoderError::SecondaryHeaderError(_)));
    }

    // ── CRC-16 computation tests (pure math) ─────────────────────────────────
    // CRC verification tests (the rule checks) live in validator.rs.

    #[test]
    fn test_crc16_known_value() {
        // CRC-16/CCITT of "123456789" (ASCII) = 0x29B1 (well-known test vector)
        let data = b"123456789";
        assert_eq!(Parser::compute_crc16(data), 0x29B1);
    }

    #[test]
    fn test_crc16_all_zeros() {
        // Known stable baseline: 8 zero bytes
        let crc = Parser::compute_crc16(&[0u8; 8]);
        assert_ne!(crc, 0); // non-zero CRC over all-zero data is expected
    }

    #[test]
    fn test_crc16_different_inputs_differ() {
        let a = Parser::compute_crc16(b"PACKET_A");
        let b = Parser::compute_crc16(b"PACKET_B");
        assert_ne!(a, b);
    }
}

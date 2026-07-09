// ── CCSDS Packet Validator ────────────────────────────────────────────────────
//
// RESPONSIBILITY: Structural rule-checking only.
//
// The Validator accepts an already-parsed `CcsdsPrimaryHeader` and the raw
// packet bytes, then asserts whether the values satisfy the CCSDS 133.0-B-2
// protocol rules.
//
// Why a separate module from parser.rs?
//
//   parser.rs  — "What are the bits?"     (reads bytes, returns values)
//   validator.rs — "Are the values legal?" (checks rules, returns errors)
//
// This boundary keeps each module small, single-purpose, and independently
// testable. When a new mission requires relaxed CRC rules or a non-standard
// version byte, only the Validator changes — the Parser stays untouched.

use crate::domain::ccsds_hdr::CcsdsPrimaryHeader;
use crate::domain::errors::DecoderError;
use crate::domain::parser::Parser;

pub struct Validator;

impl Validator {
    // ── Rule 1: CCSDS Version ────────────────────────────────────────────────

    /// Assert that the Packet Version Number is 0 (0b000).
    ///
    /// CCSDS 133.0-B-2 §4.1.2.2:
    ///   "The Packet Version Number shall be '000'."
    pub fn validate_version(header: &CcsdsPrimaryHeader) -> Result<(), DecoderError> {
        if header.version != 0 {
            return Err(DecoderError::InvalidVersion(header.version));
        }
        Ok(())
    }

    // ── Rule 2: Length Consistency ───────────────────────────────────────────

    /// Assert that the declared `packet_data_length` field matches the actual
    /// number of bytes in the data field (everything after the 6-byte header).
    ///
    /// CCSDS 133.0-B-2 §4.1.3.5.3:
    ///   "Packet Data Length = (total octets in data field) - 1"
    pub fn validate_length(data: &[u8], header: &CcsdsPrimaryHeader) -> Result<(), DecoderError> {
        let declared = header.packet_data_length as usize + 1; // actual field size
        let actual = data.len().saturating_sub(6); // bytes after primary hdr

        if declared != actual {
            return Err(DecoderError::LengthMismatch { declared, actual });
        }
        Ok(())
    }

    // ── Rule 3: CRC-16 Integrity ─────────────────────────────────────────────

    /// Assert that the CRC-16/CCITT appended at the final 2 bytes of the
    /// packet matches the computed checksum over the preceding bytes.
    ///
    /// The CRC covers `data[0..len-2]`.
    /// The stored CRC occupies `data[len-2..len]` in big-endian order.
    ///
    /// Only call this when CRC checking is enabled for the APID/mission.
    /// Returns `PacketTooShort` if there are fewer than 3 bytes in the packet
    /// (need at least 1 data byte + 2 CRC bytes to be meaningful).
    pub fn validate_crc(data: &[u8]) -> Result<(), DecoderError> {
        if data.len() < 3 {
            return Err(DecoderError::PacketTooShort(data.len()));
        }
        let payload = &data[..data.len() - 2];
        let stored_crc = u16::from_be_bytes([data[data.len() - 2], data[data.len() - 1]]);
        let computed = Parser::compute_crc16(payload);

        if computed != stored_crc {
            Err(DecoderError::CrcMismatch {
                expected: stored_crc,
                computed,
            })
        } else {
            Ok(())
        }
    }

    // ── Combined validation ───────────────────────────────────────────────────

    /// Run all structural rules in the correct order.
    ///
    /// Order is deliberate:
    ///   1. Version — cheapest check, no byte counting.
    ///   2. Length  — catches truncated packets early.
    ///   3. CRC     — most expensive (iterates all bytes); only reached if 1 & 2 pass.
    ///
    /// `check_crc`: pass `false` when the APID profile does not use a CRC.
    pub fn validate_all(
        data: &[u8],
        header: &CcsdsPrimaryHeader,
        check_crc: bool,
    ) -> Result<(), DecoderError> {
        Self::validate_version(header)?;
        Self::validate_length(data, header)?;
        if check_crc {
            Self::validate_crc(data)?;
        }
        Ok(())
    }
}

// ── Unit Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::parser::Parser;

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Build a valid 28-byte packet: 6-byte primary header + 22 bytes of data.
    /// APID=42, seq_count=1200, version=0.
    fn valid_packet() -> Vec<u8> {
        let mut pkt = vec![
            0x00, 0x2A, // word0: version=0, TM, no sec-hdr, APID=42
            0xC4, 0xB0, // word1: standalone, seq_count=1200
            0x00, 0x15, // packet_data_length=21 → data field=22 bytes
        ];
        pkt.extend_from_slice(&[0xAA; 22]);
        pkt
    }

    /// Append a correct CRC-16 to a payload slice.
    fn append_crc(payload: &[u8]) -> Vec<u8> {
        let crc = Parser::compute_crc16(payload);
        let mut pkt = payload.to_vec();
        pkt.push((crc >> 8) as u8);
        pkt.push((crc & 0xFF) as u8);
        pkt
    }

    // ── Rule 1: Version ───────────────────────────────────────────────────────

    #[test]
    fn test_validate_version_ok() {
        let pkt = valid_packet();
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        assert!(Validator::validate_version(&hdr).is_ok());
    }

    #[test]
    fn test_validate_version_invalid() {
        let mut pkt = valid_packet();
        pkt[0] |= 0xE0; // set version bits to 0b111 (= 7)
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        let err = Validator::validate_version(&hdr).unwrap_err();
        assert!(matches!(err, DecoderError::InvalidVersion(7)));
    }

    #[test]
    fn test_validate_version_any_nonzero_value_rejected() {
        for bad_version in [1u8, 2, 3, 4, 5, 6, 7] {
            let mut pkt = valid_packet();
            // Clear the top 3 bits then set the bad version
            pkt[0] = (pkt[0] & 0x1F) | ((bad_version & 0x07) << 5);
            let hdr = Parser::parse_primary_header(&pkt).unwrap();
            assert!(
                Validator::validate_version(&hdr).is_err(),
                "version {bad_version} should be rejected"
            );
        }
    }

    // ── Rule 2: Length ────────────────────────────────────────────────────────

    #[test]
    fn test_validate_length_ok() {
        let pkt = valid_packet();
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        assert!(Validator::validate_length(&pkt, &hdr).is_ok());
    }

    #[test]
    fn test_validate_length_too_short() {
        let pkt = valid_packet();
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        let short = &pkt[..pkt.len() - 4]; // lop 4 bytes off
        let err = Validator::validate_length(short, &hdr).unwrap_err();
        assert!(matches!(
            err,
            DecoderError::LengthMismatch {
                declared: 22,
                actual: 18
            }
        ));
    }

    #[test]
    fn test_validate_length_too_long() {
        let mut pkt = valid_packet();
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        pkt.extend_from_slice(&[0xFF; 3]); // append extra bytes
        let err = Validator::validate_length(&pkt, &hdr).unwrap_err();
        assert!(matches!(
            err,
            DecoderError::LengthMismatch {
                declared: 22,
                actual: 25
            }
        ));
    }

    // ── Rule 3: CRC ───────────────────────────────────────────────────────────

    #[test]
    fn test_validate_crc_ok() {
        let payload = b"MUST_CCSDS_DECODER_TEST";
        let pkt = append_crc(payload);
        assert!(Validator::validate_crc(&pkt).is_ok());
    }

    #[test]
    fn test_validate_crc_corrupted_byte() {
        let payload = b"MUST_CCSDS_DECODER_TEST";
        let mut pkt = append_crc(payload);
        pkt[5] ^= 0xFF; // flip a bit in the middle of the payload
        let err = Validator::validate_crc(&pkt).unwrap_err();
        assert!(matches!(err, DecoderError::CrcMismatch { .. }));
    }

    #[test]
    fn test_validate_crc_corrupted_stored_crc() {
        let payload = b"MUST_CCSDS_DECODER_TEST";
        let mut pkt = append_crc(payload);
        let last = pkt.len() - 1;
        pkt[last] ^= 0xFF; // flip a bit in the stored CRC itself
        let err = Validator::validate_crc(&pkt).unwrap_err();
        assert!(matches!(err, DecoderError::CrcMismatch { .. }));
    }

    #[test]
    fn test_validate_crc_single_byte_payload() {
        let pkt = append_crc(b"\xDE");
        assert!(Validator::validate_crc(&pkt).is_ok());
    }

    #[test]
    fn test_validate_crc_too_short_packet() {
        let pkt = append_crc(b""); // 0-byte payload → 2-byte total (only CRC)
        let err = Validator::validate_crc(&pkt).unwrap_err();
        assert!(matches!(err, DecoderError::PacketTooShort(2)));
    }

    // ── Combined ──────────────────────────────────────────────────────────────

    #[test]
    fn test_validate_all_ok_without_crc() {
        let pkt = valid_packet();
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        assert!(Validator::validate_all(&pkt, &hdr, false).is_ok());
    }

    #[test]
    fn test_validate_all_ok_with_crc() {
        // Build a valid packet that also carries a correct CRC at the end.
        // Primary header: version=0, APID=7, data_length = payload_len+crc_len-1
        let payload = b"HELLO_MUST"; // 10 bytes payload
                                     // We'll encode a minimal packet: 6 bytes header + 10 bytes payload + 2 CRC = 18 total
                                     // data_field_size = 10 + 2 = 12, packet_data_length = 11 (0x000B)
        let mut pkt = vec![0x00, 0x07, 0xC0, 0x00, 0x00, 0x0B];
        pkt.extend_from_slice(payload);
        // Compute CRC over everything so far
        let crc = Parser::compute_crc16(&pkt);
        pkt.push((crc >> 8) as u8);
        pkt.push((crc & 0xFF) as u8);
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        assert!(Validator::validate_all(&pkt, &hdr, true).is_ok());
    }

    #[test]
    fn test_validate_all_fails_on_bad_version_first() {
        // Even if length and CRC are fine, bad version should fail
        let mut pkt = valid_packet();
        pkt[0] |= 0xE0;
        let hdr = Parser::parse_primary_header(&pkt).unwrap();
        let err = Validator::validate_all(&pkt, &hdr, false).unwrap_err();
        assert!(matches!(err, DecoderError::InvalidVersion(7)));
    }
}

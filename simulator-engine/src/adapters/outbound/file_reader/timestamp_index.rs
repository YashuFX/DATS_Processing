use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use crate::domain::errors::ReplayError;

/// A single entry in the eager index.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexEntry {
    pub file_offset: u64,
    pub timestamp_ns: u64,
}

/// The index containing all packet offsets and timestamps.
#[derive(Debug, Clone, Default)]
pub struct TimestampIndex {
    pub entries: Vec<IndexEntry>,
    pub total_packets: u64,
    pub start_timestamp_ns: u64,
    pub end_timestamp_ns: u64,
    pub duration_ns: u64,
}

pub const BINARY_SYNC_WORD: [u8; 4] = [0x1A, 0x2B, 0x3C, 0x4D];

impl TimestampIndex {
    /// Builds an index by scanning a file from start to finish.
    /// Supports "binary" (with sync words) and "ccsds" formats.
    pub fn build(file_path: &str, file_type: &str) -> Result<Self, ReplayError> {
        let mut file = File::open(file_path)
            .map_err(|e| ReplayError::FileIo(format!("Failed to open file: {}", e)))?;
        
        let file_len = file.metadata()
            .map_err(|e| ReplayError::FileIo(format!("Failed to get file metadata: {}", e)))?
            .len();

        let mut entries = Vec::new();
        let mut offset = 0;

        match file_type {
            "ccsds" => {
                let mut header = [0u8; 6];
                let mut ts_buf = [0u8; 8];

                while offset + 6 <= file_len {
                    file.seek(SeekFrom::Start(offset))
                        .map_err(|e| ReplayError::FileIo(e.to_string()))?;
                    
                    file.read_exact(&mut header)
                        .map_err(|e| ReplayError::FileIo(e.to_string()))?;

                    // CCSDS Packet Length field is at bytes 4-5
                    let len_field = u16::from_be_bytes([header[4], header[5]]) as u64;
                    // Total CCSDS packet length is length_field + 7
                    let packet_len = len_field + 7;

                    if offset + packet_len > file_len {
                        return Err(ReplayError::PacketCorruption(format!(
                            "Truncated CCSDS packet at offset {}: expected {} bytes, but file ends early",
                            offset, packet_len
                        )));
                    }

                    // Check secondary header flag (bit 3 of byte 0, mask 0x08)
                    let has_sec_hdr = (header[0] & 0x08) != 0;
                    if !has_sec_hdr {
                        return Err(ReplayError::PacketCorruption(format!(
                            "CCSDS packet at offset {} is missing secondary time header flag",
                            offset
                        )));
                    }

                    // Read 8-byte timestamp from secondary header (which starts at byte 6)
                    file.read_exact(&mut ts_buf)
                        .map_err(|e| ReplayError::FileIo(e.to_string()))?;

                    let timestamp_ns = u64::from_be_bytes(ts_buf);

                    entries.push(IndexEntry {
                        file_offset: offset,
                        timestamp_ns,
                    });

                    offset += packet_len;
                }
            }
            "binary" => {
                let mut header = [0u8; 6]; // 4 bytes sync + 2 bytes length
                let mut ts_buf = [0u8; 8]; // 8 bytes timestamp

                while offset + 14 <= file_len {
                    file.seek(SeekFrom::Start(offset))
                        .map_err(|e| ReplayError::FileIo(e.to_string()))?;

                    file.read_exact(&mut header)
                        .map_err(|e| ReplayError::FileIo(e.to_string()))?;

                    // Check for binary sync word
                    if header[0..4] != BINARY_SYNC_WORD {
                        // Scan forward byte-by-byte for next sync word
                        offset += 1;
                        continue;
                    }

                    let payload_len = u16::from_be_bytes([header[4], header[5]]) as u64;
                    let packet_len = 4 + 2 + 8 + payload_len; // sync + len + ts + payload

                    if offset + packet_len > file_len {
                        return Err(ReplayError::PacketCorruption(format!(
                            "Truncated binary packet at offset {}: expected {} bytes, but file ends early",
                            offset, packet_len
                        )));
                    }

                    file.read_exact(&mut ts_buf)
                        .map_err(|e| ReplayError::FileIo(e.to_string()))?;

                    let timestamp_ns = u64::from_be_bytes(ts_buf);

                    entries.push(IndexEntry {
                        file_offset: offset,
                        timestamp_ns,
                    });

                    offset += packet_len;
                }
            }
            _ => return Err(ReplayError::Configuration(format!("Unsupported file type: {}", file_type))),
        }

        if entries.is_empty() {
            return Err(ReplayError::PacketCorruption("No valid packets found in file".to_string()));
        }

        let start_timestamp_ns = entries[0].timestamp_ns;
        let end_timestamp_ns = entries[entries.len() - 1].timestamp_ns;
        // Non-monotonic check is handled gracefully, but we can compute duration:
        let duration_ns = if end_timestamp_ns >= start_timestamp_ns {
            end_timestamp_ns - start_timestamp_ns
        } else {
            0
        };

        Ok(Self {
            total_packets: entries.len() as u64,
            start_timestamp_ns,
            end_timestamp_ns,
            duration_ns,
            entries,
        })
    }

    /// Finds the entry index nearest to the target timestamp using binary search.
    pub fn find_nearest_index(&self, target_ns: u64) -> usize {
        if self.entries.is_empty() {
            return 0;
        }
        
        match self.entries.binary_search_by_key(&target_ns, |entry| entry.timestamp_ns) {
            Ok(idx) => idx,
            Err(idx) => {
                if idx == 0 {
                    0
                } else if idx >= self.entries.len() {
                    self.entries.len() - 1
                } else {
                    // Choose the nearest one
                    let diff_prev = target_ns.saturating_sub(self.entries[idx - 1].timestamp_ns);
                    let diff_next = self.entries[idx].timestamp_ns.saturating_sub(target_ns);
                    if diff_prev < diff_next {
                        idx - 1
                    } else {
                        idx
                    }
                }
            }
        }
    }
}

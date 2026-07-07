use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use crate::domain::errors::ReplayError;
use crate::domain::file_format::{ReplayFileFormat, BINARY_SYNC_WORD, BINARY_FRAME_HEADER_LEN, CCSDS_PRIMARY_HEADER_LEN};
use crate::domain::replay_packet::ReplayPacket;

/// A streaming packet iterator that reads packets sequentially from a telemetry
/// recording file. Implements `Iterator` for ergonomic consumption.
///
/// ## Usage
///
/// ```rust,no_run
/// let iter = PacketIterator::open("pass.bin", ReplayFileFormat::Binary)?;
/// for result in iter {
///     match result {
///         Ok(packet) => println!("Packet #{} at ts={}", packet.sequence_number, packet.original_timestamp_ns),
///         Err(e) => eprintln!("Read error: {}", e),
///     }
/// }
/// ```
pub struct PacketIterator {
    reader: BufReader<File>,
    format: ReplayFileFormat,
    file_length: u64,
    current_offset: u64,
    sequence_counter: u64,
    packets_read: u64,
    finished: bool,
}

impl PacketIterator {
    /// Opens a telemetry file and creates a new PacketIterator.
    pub fn open(path: &str, format: ReplayFileFormat) -> Result<Self, ReplayError> {
        if path.contains("..") {
            return Err(ReplayError::Configuration("Path traversal attempt detected".to_string()));
        }

        let file = File::open(path)
            .map_err(|e| ReplayError::FileIo(format!("Failed to open '{}': {}", path, e)))?;

        let file_length = file.metadata()
            .map_err(|e| ReplayError::FileIo(format!("Failed to get file metadata: {}", e)))?
            .len();

        let reader = BufReader::with_capacity(8 * 1024 * 1024, file); // 8 MB read buffer

        Ok(Self {
            reader,
            format,
            file_length,
            current_offset: 0,
            sequence_counter: 0,
            packets_read: 0,
            finished: false,
        })
    }

    /// Returns the number of packets successfully read so far.
    pub fn packets_read(&self) -> u64 {
        self.packets_read
    }

    /// Returns the current byte offset in the file.
    pub fn current_offset(&self) -> u64 {
        self.current_offset
    }

    /// Seeks to a specific byte offset in the file.
    /// After seeking, the iterator will read packets starting from this offset.
    pub fn seek_to_offset(&mut self, offset: u64) -> Result<(), ReplayError> {
        self.reader.seek(SeekFrom::Start(offset))
            .map_err(|e| ReplayError::FileIo(format!("Seek to offset {} failed: {}", offset, e)))?;
        self.current_offset = offset;
        self.finished = false;
        Ok(())
    }

    /// Reads the next Binary-format packet.
    fn read_next_binary(&mut self) -> Result<Option<ReplayPacket>, ReplayError> {
        // Need at least 14 bytes for a minimal frame (sync + len + ts + 0-byte payload)
        if self.current_offset + BINARY_FRAME_HEADER_LEN as u64 > self.file_length {
            return Ok(None);
        }

        // Read the 6-byte header (sync + length)
        let mut header = [0u8; 6];
        if let Err(e) = self.reader.read_exact(&mut header) {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                return Ok(None);
            }
            return Err(ReplayError::FileIo(format!("Failed to read binary header: {}", e)));
        }

        // Validate sync word
        if header[0..4] != BINARY_SYNC_WORD {
            // Lost sync — advance 1 byte and try to resync
            self.current_offset += 1;
            self.reader.seek(SeekFrom::Start(self.current_offset))
                .map_err(|e| ReplayError::FileIo(format!("Resync seek failed: {}", e)))?;
            return Err(ReplayError::PacketCorruption(format!(
                "Invalid sync word at offset {}; attempting resync", self.current_offset - 1
            )));
        }

        let payload_len = u16::from_be_bytes([header[4], header[5]]) as usize;

        // Read 8-byte timestamp
        let mut ts_buf = [0u8; 8];
        self.reader.read_exact(&mut ts_buf)
            .map_err(|e| ReplayError::FileIo(format!("Failed to read timestamp: {}", e)))?;
        let timestamp_ns = u64::from_be_bytes(ts_buf);

        // Read payload
        let mut payload = vec![0u8; payload_len];
        if payload_len > 0 {
            self.reader.read_exact(&mut payload)
                .map_err(|e| ReplayError::FileIo(format!("Failed to read payload: {}", e)))?;
        }

        // Build the complete raw frame
        let frame_len = BINARY_FRAME_HEADER_LEN + payload_len;
        let mut raw_data = Vec::with_capacity(frame_len);
        raw_data.extend_from_slice(&header);
        raw_data.extend_from_slice(&ts_buf);
        raw_data.extend_from_slice(&payload);

        let file_offset = self.current_offset;
        self.current_offset += frame_len as u64;
        self.sequence_counter += 1;
        self.packets_read += 1;

        Ok(Some(ReplayPacket::from_binary_frame(
            raw_data,
            timestamp_ns,
            file_offset,
            self.sequence_counter,
        )))
    }

    /// Reads the next CCSDS-format packet.
    fn read_next_ccsds(&mut self) -> Result<Option<ReplayPacket>, ReplayError> {
        // Need at least 6 bytes for the primary header
        if self.current_offset + CCSDS_PRIMARY_HEADER_LEN as u64 > self.file_length {
            return Ok(None);
        }

        // Read 6-byte primary header
        let mut header = [0u8; 6];
        if let Err(e) = self.reader.read_exact(&mut header) {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                return Ok(None);
            }
            return Err(ReplayError::FileIo(format!("Failed to read CCSDS header: {}", e)));
        }

        // Parse data length field
        let data_length = u16::from_be_bytes([header[4], header[5]]) as usize;
        let data_field_len = data_length + 1; // CCSDS convention: field = actual_size - 1

        // Validate secondary header flag
        let has_sec_hdr = (header[0] & 0x08) != 0;
        if !has_sec_hdr {
            return Err(ReplayError::PacketCorruption(format!(
                "CCSDS packet at offset {} missing secondary header flag", self.current_offset
            )));
        }

        // Read the data field
        let mut data_field = vec![0u8; data_field_len];
        self.reader.read_exact(&mut data_field)
            .map_err(|e| ReplayError::FileIo(format!("Failed to read CCSDS data field: {}", e)))?;

        // Extract 8-byte timestamp from the start of the data field (secondary header)
        let timestamp_ns = if data_field.len() >= 8 {
            u64::from_be_bytes([
                data_field[0], data_field[1], data_field[2], data_field[3],
                data_field[4], data_field[5], data_field[6], data_field[7],
            ])
        } else {
            return Err(ReplayError::TimestampCorruption(format!(
                "CCSDS secondary header too short at offset {}", self.current_offset
            )));
        };

        // Build the complete raw packet
        let mut raw_data = Vec::with_capacity(CCSDS_PRIMARY_HEADER_LEN + data_field_len);
        raw_data.extend_from_slice(&header);
        raw_data.extend_from_slice(&data_field);

        let file_offset = self.current_offset;
        let packet_len = CCSDS_PRIMARY_HEADER_LEN + data_field_len;
        self.current_offset += packet_len as u64;
        self.sequence_counter += 1;
        self.packets_read += 1;

        Ok(Some(ReplayPacket::from_ccsds_frame(
            raw_data,
            timestamp_ns,
            file_offset,
            self.sequence_counter,
        )))
    }
}

impl Iterator for PacketIterator {
    type Item = Result<ReplayPacket, ReplayError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        let result = match self.format {
            ReplayFileFormat::Binary => self.read_next_binary(),
            ReplayFileFormat::Ccsds => self.read_next_ccsds(),
        };

        match result {
            Ok(Some(packet)) => Some(Ok(packet)),
            Ok(None) => {
                self.finished = true;
                None
            }
            Err(e) => {
                // On PacketCorruption, we return the error but let the caller decide
                // whether to continue (by calling next() again) or stop.
                if matches!(e, ReplayError::PacketCorruption(_)) {
                    Some(Err(e))
                } else {
                    // Fatal errors stop the iterator
                    self.finished = true;
                    Some(Err(e))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_temp_binary_file(packets: &[(u64, &[u8])]) -> String {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("rss_test_{}.bin", uuid::Uuid::new_v4()));
        let mut file = File::create(&path).unwrap();

        for (timestamp_ns, payload) in packets {
            // Sync word
            file.write_all(&BINARY_SYNC_WORD).unwrap();
            // Payload length
            file.write_all(&(payload.len() as u16).to_be_bytes()).unwrap();
            // Timestamp
            file.write_all(&timestamp_ns.to_be_bytes()).unwrap();
            // Payload
            file.write_all(payload).unwrap();
        }

        path.to_string_lossy().to_string()
    }

    #[test]
    fn test_binary_iterator_reads_all_packets() {
        let packets = vec![
            (1_000_000_000u64, b"PKT_ONE".as_slice()),
            (2_000_000_000u64, b"PKT_TWO".as_slice()),
            (3_000_000_000u64, b"PKT_THREE".as_slice()),
        ];
        let path = create_temp_binary_file(&packets);

        let iter = PacketIterator::open(&path, ReplayFileFormat::Binary).unwrap();
        let results: Vec<_> = iter.collect();

        assert_eq!(results.len(), 3);

        let pkt1 = results[0].as_ref().unwrap();
        assert_eq!(pkt1.sequence_number, 1);
        assert_eq!(pkt1.original_timestamp_ns, 1_000_000_000);
        assert_eq!(pkt1.payload, b"PKT_ONE");

        let pkt2 = results[1].as_ref().unwrap();
        assert_eq!(pkt2.sequence_number, 2);
        assert_eq!(pkt2.original_timestamp_ns, 2_000_000_000);
        assert_eq!(pkt2.payload, b"PKT_TWO");

        let pkt3 = results[2].as_ref().unwrap();
        assert_eq!(pkt3.sequence_number, 3);
        assert_eq!(pkt3.original_timestamp_ns, 3_000_000_000);
        assert_eq!(pkt3.payload, b"PKT_THREE");

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn test_binary_iterator_empty_file() {
        let path = create_temp_binary_file(&[]);
        let iter = PacketIterator::open(&path, ReplayFileFormat::Binary).unwrap();
        let results: Vec<_> = iter.collect();
        assert!(results.is_empty());
        std::fs::remove_file(path).ok();
    }
}

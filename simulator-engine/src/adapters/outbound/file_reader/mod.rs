use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use crate::ports::SourcePort;
use crate::domain::models::SourceMetadata;
use crate::domain::replay_packet::ReplayPacket;
use crate::domain::errors::ReplayError;
use crate::adapters::outbound::file_reader::timestamp_index::{TimestampIndex, BINARY_SYNC_WORD};

pub mod timestamp_index;

pub struct FileReaderAdapter {
    file_type: String, // "binary" or "ccsds"
    file: Option<File>,
    index: Option<TimestampIndex>,
    current_index: usize,
    file_path: Option<String>,
}

impl FileReaderAdapter {
    /// Creates a new FileReaderAdapter for a specific file type.
    pub fn new(file_type: &str) -> Self {
        Self {
            file_type: file_type.to_lowercase(),
            file: None,
            index: None,
            current_index: 0,
            file_path: None,
        }
    }
}

impl SourcePort for FileReaderAdapter {
    fn open(&mut self, path: &str, file_type: &str) -> Result<SourceMetadata, ReplayError> {
        // Path traversal protection: reject paths containing ".."
        if path.contains("..") {
            return Err(ReplayError::Configuration("Path traversal attempt detected".to_string()));
        }

        self.file_type = file_type.to_lowercase();

        let file = File::open(path)
            .map_err(|e| ReplayError::FileIo(format!("Failed to open file '{}': {}", path, e)))?;

        let file_size_bytes = file.metadata()
            .map_err(|e| ReplayError::FileIo(format!("Failed to read file size: {}", e)))?
            .len();

        // Build the eager timestamp index
        let index = TimestampIndex::build(path, &self.file_type)?;

        let metadata = SourceMetadata {
            total_packets: index.total_packets,
            duration_ns: index.duration_ns,
            start_timestamp_ns: index.start_timestamp_ns,
            end_timestamp_ns: index.end_timestamp_ns,
            file_size_bytes,
        };

        self.file = Some(file);
        self.index = Some(index);
        self.current_index = 0;
        self.file_path = Some(path.to_string());

        Ok(metadata)
    }

    fn read_next_packet(&mut self) -> Result<Option<ReplayPacket>, ReplayError> {
        let file = self.file.as_mut().ok_or_else(|| {
            ReplayError::FileIo("Cannot read from file: reader is not open".to_string())
        })?;

        let index = self.index.as_ref().ok_or_else(|| {
            ReplayError::FileIo("Reader index is not initialized".to_string())
        })?;

        if self.current_index >= index.entries.len() {
            return Ok(None); // EOF
        }

        let entry = &index.entries[self.current_index];
        file.seek(SeekFrom::Start(entry.file_offset))
            .map_err(|e| ReplayError::FileIo(format!("Seek failed: {}", e)))?;

        let mut packet_data = Vec::new();

        match self.file_type.as_str() {
            "ccsds" => {
                let mut header = [0u8; 6];
                file.read_exact(&mut header)
                    .map_err(|e| ReplayError::FileIo(format!("Failed to read CCSDS header: {}", e)))?;

                let len_field = u16::from_be_bytes([header[4], header[5]]) as usize;
                let data_field_len = len_field + 1;

                let mut data_field = vec![0u8; data_field_len];
                file.read_exact(&mut data_field)
                    .map_err(|e| ReplayError::FileIo(format!("Failed to read CCSDS packet data field: {}", e)))?;

                packet_data.extend_from_slice(&header);
                packet_data.extend_from_slice(&data_field);
            }
            "binary" => {
                let mut header = [0u8; 6]; // 4 bytes sync + 2 bytes length
                file.read_exact(&mut header)
                    .map_err(|e| ReplayError::FileIo(format!("Failed to read binary header: {}", e)))?;

                if header[0..4] != BINARY_SYNC_WORD {
                    return Err(ReplayError::PacketCorruption(format!(
                        "Invalid sync word at offset {}", entry.file_offset
                    )));
                }

                let payload_len = u16::from_be_bytes([header[4], header[5]]) as usize;
                let remaining_len = 8 + payload_len; // 8 bytes timestamp + payload

                let mut remaining = vec![0u8; remaining_len];
                file.read_exact(&mut remaining)
                    .map_err(|e| ReplayError::FileIo(format!("Failed to read binary payload: {}", e)))?;

                packet_data.extend_from_slice(&header);
                packet_data.extend_from_slice(&remaining);
            }
            _ => return Err(ReplayError::Configuration(format!("Unsupported file type: {}", self.file_type))),
        }

        let seq_num = (self.current_index + 1) as u64;
        let packet = match self.file_type.as_str() {
            "ccsds" => ReplayPacket::from_ccsds_frame(
                packet_data,
                entry.timestamp_ns,
                entry.file_offset,
                seq_num,
            ),
            _ => ReplayPacket::from_binary_frame(
                packet_data,
                entry.timestamp_ns,
                entry.file_offset,
                seq_num,
            ),
        };

        self.current_index += 1;
        Ok(Some(packet))
    }

    fn seek(&mut self, timestamp_ns: u64) -> Result<(), ReplayError> {
        let index = self.index.as_ref().ok_or_else(|| {
            ReplayError::FileIo("Cannot seek: reader index is not initialized".to_string())
        })?;

        self.current_index = index.find_nearest_index(timestamp_ns);
        Ok(())
    }

    fn position(&self) -> u64 {
        self.current_index as u64
    }

    fn close(&mut self) -> Result<(), ReplayError> {
        self.file = None;
        self.index = None;
        self.current_index = 0;
        self.file_path = None;
        Ok(())
    }

    fn metadata(&self) -> Option<SourceMetadata> {
        let index = self.index.as_ref()?;
        let file_path = self.file_path.as_ref()?;
        let file = File::open(file_path).ok()?;
        let file_size_bytes = file.metadata().ok()?.len();

        Some(SourceMetadata {
            total_packets: index.total_packets,
            duration_ns: index.duration_ns,
            start_timestamp_ns: index.start_timestamp_ns,
            end_timestamp_ns: index.end_timestamp_ns,
            file_size_bytes,
        })
    }
}

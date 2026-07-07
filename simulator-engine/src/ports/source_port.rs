use crate::domain::models::SourceMetadata;
use crate::domain::replay_packet::ReplayPacket;
use crate::domain::errors::ReplayError;

pub trait SourcePort: Send + Sync {
    /// Opens the telemetry source file and performs eager indexing.
    fn open(&mut self, path: &str, file_type: &str) -> Result<SourceMetadata, ReplayError>;

    /// Reads the next packet from the file stream.
    /// Returns Ok(None) upon reaching EOF.
    fn read_next_packet(&mut self) -> Result<Option<ReplayPacket>, ReplayError>;

    /// Seeks to the packet nearest to the target timestamp in nanoseconds.
    fn seek(&mut self, timestamp_ns: u64) -> Result<(), ReplayError>;

    /// Returns the current packet index / position.
    fn position(&self) -> u64;

    /// Closes the file reader and releases resources.
    fn close(&mut self) -> Result<(), ReplayError>;

    /// Retrieves metadata if the file is loaded.
    fn metadata(&self) -> Option<SourceMetadata>;
}

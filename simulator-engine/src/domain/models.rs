use serde::{Deserialize, Serialize};

/// Metadata about a telemetry source file, computed during eager indexing.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SourceMetadata {
    /// Total number of packets found in the file.
    pub total_packets: u64,
    
    /// Duration of the pass in nanoseconds (last_timestamp - first_timestamp).
    pub duration_ns: u64,
    
    /// Start timestamp of the pass in nanoseconds (monotonic/relative epoch).
    pub start_timestamp_ns: u64,
    
    /// End timestamp of the pass in nanoseconds.
    pub end_timestamp_ns: u64,
    
    /// Size of the file in bytes.
    pub file_size_bytes: u64,
}

/// Logical playback clock statistics reported during queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackStats {
    pub packets_published: u64,
    pub current_timestamp_ns: u64,
    pub progress: f64,
}

use std::fmt;

#[derive(Debug, Clone)]
pub enum ReplayError {
    Configuration(String),
    FileIo(String),
    PacketCorruption(String),
    TimestampCorruption(String),
    InvalidTransition { current: String, event: String },
    Network(String),
    Eof,
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(msg) => write!(f, "Configuration error: {}", msg),
            Self::FileIo(msg) => write!(f, "File I/O error: {}", msg),
            Self::PacketCorruption(msg) => write!(f, "Packet corruption: {}", msg),
            Self::TimestampCorruption(msg) => write!(f, "Timestamp corruption: {}", msg),
            Self::InvalidTransition { current, event } => {
                write!(f, "Invalid state transition: cannot trigger '{}' in state '{}'", event, current)
            }
            Self::Network(msg) => write!(f, "Network error: {}", msg),
            Self::Eof => write!(f, "End of file reached"),
        }
    }
}

impl std::error::Error for ReplayError {}

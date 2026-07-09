use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum DomainError {
    #[error("Failed to load registry file: {0}")]
    RegistryLoadError(String),

    #[error("Failed to parse registry configuration: {0}")]
    RegistryParseError(String),

    #[error("No matching mission rule found for source_id: '{source_id}', apid: {apid}, vcid: {vcid:?}")]
    UnidentifiedPacket {
        source_id: String,
        apid: u32,
        vcid: Option<u32>,
    },

    #[error("Ambiguous mission rules matched multiple missions/satellites for source_id: '{source_id}', apid: {apid}")]
    AmbiguousMatch {
        source_id: String,
        apid: u32,
    },
}

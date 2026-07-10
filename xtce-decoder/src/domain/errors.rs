use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum XtceError {
    #[error("Configuration error: {0}")]
    ConfigError(String),
    #[error("XML Parse error: {0}")]
    XmlParseError(String),
    #[error("XML Validation error: {0}")]
    XmlValidationError(String),
    #[error("Container not found for APID {0}")]
    ContainerNotFound(u32),
    #[error("Decommutation error: {0}")]
    DecommutationFailed(String),
    #[error("Calibration error: {0}")]
    CalibrationFailed(String),
}

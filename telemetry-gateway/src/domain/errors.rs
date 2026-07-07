use std::fmt;

#[derive(Debug, Clone)]
pub enum GatewayError {
    ValidationError(String),
    RegistryError(String),
    SessionError(String),
    PublishError(String),
    Internal(String),
}

impl std::error::Error for GatewayError {}

impl fmt::Display for GatewayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GatewayError::ValidationError(msg) => write!(f, "Validation Error: {}", msg),
            GatewayError::RegistryError(msg) => write!(f, "Registry Error: {}", msg),
            GatewayError::SessionError(msg) => write!(f, "Session Error: {}", msg),
            GatewayError::PublishError(msg) => write!(f, "Publish Error: {}", msg),
            GatewayError::Internal(msg) => write!(f, "Internal Error: {}", msg),
        }
    }
}

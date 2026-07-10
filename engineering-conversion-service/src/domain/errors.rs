use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    #[error("Configuration directory not found: {0}")]
    ConfigDirNotFound(String),

    #[error("Configuration file not found for mission {0}: {1}")]
    ConfigFileNotFound(String, String),

    #[error("Failed to read configuration for mission {0}: {1}")]
    ConfigReadError(String, String),

    #[error("Failed to parse YAML configuration for mission {0}: {1}")]
    ConfigParseError(String, String),

    #[error("Cyclic dependency detected in derived parameters for mission {0}: {1}")]
    CyclicDependency(String, String),

    #[error("Duplicate parameter definition for parameter: {0}")]
    DuplicateParameter(String),

    #[error("Invalid math expression in formula '{0}': {1}")]
    InvalidExpression(String, String),

    #[error("Missing input parameter '{0}' for derived parameter '{1}'")]
    MissingInputParameter(String, String),

    #[error("Evaluation error in formula '{0}': {1}")]
    EvaluationError(String, String),

    #[error("Type conversion error: {0}")]
    TypeConversionError(String),
}

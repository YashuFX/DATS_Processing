use crate::domain::models::Mission;
use crate::domain::errors::DomainError;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct RegistryConfig {
    pub missions: Vec<Mission>,
}

#[derive(Debug, Clone)]
pub struct MissionRegistry {
    config: RegistryConfig,
}

impl MissionRegistry {
    pub fn from_yaml(content: &str) -> Result<Self, DomainError> {
        let config: RegistryConfig = serde_yaml::from_str(content)
            .map_err(|e| DomainError::RegistryParseError(e.to_string()))?;
        Ok(Self { config })
    }

    pub fn missions(&self) -> &[Mission] {
        &self.config.missions
    }
}

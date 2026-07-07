use crate::domain::errors::GatewayError;
use crate::domain::models::{SourceRegistration, Session};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RegisterSourceRequest {
    pub source_type: String,
    pub source_name: String,
    pub mission_id: String,
    pub mission_name: String,
    pub satellite_id: String,
    pub satellite_name: String,
    pub station_id: String,
    pub station_name: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RegisterSourceResponse {
    pub source_id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GatewayStatus {
    pub active_sessions: usize,
    pub active_sources: usize,
    pub total_packets_received: u64,
    pub total_packets_published: u64,
    pub total_packets_rejected: u64,
}

#[tonic::async_trait]
pub trait ControlPort: Send + Sync {
    async fn register_source(&self, req: RegisterSourceRequest) -> Result<RegisterSourceResponse, GatewayError>;
    async fn unregister_source(&self, source_id: &str) -> Result<(), GatewayError>;
    async fn stop_session(&self, session_id: &str) -> Result<(), GatewayError>;
    async fn get_status(&self) -> Result<GatewayStatus, GatewayError>;
    async fn get_sessions(&self) -> Result<Vec<Session>, GatewayError>;
    async fn get_registrations(&self) -> Result<Vec<SourceRegistration>, GatewayError>;
}

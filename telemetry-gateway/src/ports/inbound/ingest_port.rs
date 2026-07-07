use crate::api::telemetry::v1::TelemetryEnvelope;
use crate::domain::errors::GatewayError;

#[tonic::async_trait]
pub trait IngestPort: Send + Sync {
    async fn on_packet_received(&self, envelope: TelemetryEnvelope) -> Result<(), GatewayError>;
    async fn on_source_connected(&self, source_id: &str) -> Result<(), GatewayError>;
    async fn on_source_disconnected(&self, source_id: &str) -> Result<(), GatewayError>;
    async fn on_session_eof(&self, session_id: &str) -> Result<(), GatewayError>;
}

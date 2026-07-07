use crate::api::telemetry::v1::TelemetryEnvelope;
use crate::domain::errors::GatewayError;

#[tonic::async_trait]
pub trait PublishPort: Send + Sync {
    async fn publish(&self, envelope: TelemetryEnvelope, routing_key: &str) -> Result<(), GatewayError>;
    fn is_connected(&self) -> bool;
    fn buffer_depth(&self) -> usize;
}

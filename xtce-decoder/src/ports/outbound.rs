use async_trait::async_trait;
use crate::domain::errors::XtceError;
use crate::proto::TelemetryEnvelope;

#[async_trait]
pub trait EngineeringPublisher: Send + Sync {
    /// Publishes the enriched telemetry envelope to the outbound exchange with the specified routing key.
    async fn publish(&self, envelope: &TelemetryEnvelope, routing_key: &str) -> Result<(), XtceError>;
}

#[async_trait]
pub trait AlertPort: Send + Sync {
    /// Emits a warning log/alert.
    async fn emit_warning(&self, context: &str, message: &str);
    /// Emits a critical failure log/alert.
    async fn emit_critical(&self, context: &str, message: &str);
}

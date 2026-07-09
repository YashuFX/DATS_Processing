use async_trait::async_trait;
use crate::domain::errors::DomainError;
use crate::proto::TelemetryEnvelope;

#[async_trait]
pub trait IdentifiedPublisher: Send + Sync {
    async fn publish(
        &self,
        envelope: &TelemetryEnvelope,
        routing_key: &str,
    ) -> Result<(), DomainError>;
}

#[async_trait]
pub trait AlertPort: Send + Sync {
    async fn alert_unidentified(&self, source_id: &str, apid: u32, vcid: Option<u32>);
    async fn alert_ambiguous(&self, source_id: &str, apid: u32);
}

use crate::api::events::v1::PlatformEvent;
use crate::domain::errors::GatewayError;

#[tonic::async_trait]
pub trait EventPort: Send + Sync {
    async fn emit(&self, event: PlatformEvent) -> Result<(), GatewayError>;
}

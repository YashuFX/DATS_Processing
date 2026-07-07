use crate::api::events::v1::PlatformEvent;
use crate::domain::errors::ReplayError;

pub trait EventPort: Send + Sync {
    /// Emits a platform state change or warning event.
    fn emit(&self, event: PlatformEvent) -> Result<(), ReplayError>;
}

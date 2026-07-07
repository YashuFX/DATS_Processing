use crate::api::telemetry::v1::TelemetryEnvelope;
use crate::domain::errors::ReplayError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackpressureStatus {
    Normal,
    HighWatermark,
}

pub trait PublishPort: Send + Sync {
    /// Publishes a normalized, enveloped packet to the downstream telemetry receiver / event bus.
    fn publish(&self, envelope: TelemetryEnvelope) -> Result<(), ReplayError>;

    /// Returns true if the connection to downstream is active.
    fn is_connected(&self) -> bool;

    /// Checks the queue buffer saturation levels.
    fn backpressure_status(&self) -> BackpressureStatus;
}

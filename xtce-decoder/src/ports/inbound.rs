use std::sync::Arc;
use async_trait::async_trait;
use futures::future::BoxFuture;
use crate::domain::errors::XtceError;

/// A handle the consumer gives to the handler so it can ACK or NACK the
/// original AMQP delivery AFTER the handler has finished processing.
pub struct DeliveryAcker {
    inner: Box<dyn AckerInner + Send>,
}

/// Private trait implemented by the RabbitMQ adapter.
#[async_trait]
pub(crate) trait AckerInner {
    async fn ack(&mut self);
    async fn nack(&mut self);
}

impl DeliveryAcker {
    /// Create a new `DeliveryAcker` from any type implementing `AckerInner`.
    pub(crate) fn new(inner: Box<dyn AckerInner + Send>) -> Self {
        Self { inner }
    }

    /// ACK the original delivery — message removed from queue.
    pub async fn ack(mut self) {
        self.inner.ack().await;
    }

    /// NACK the original delivery — message discarded (no-requeue).
    pub async fn nack(mut self) {
        self.inner.nack().await;
    }
}

/// Handler function type.
/// - `raw_bytes` — the raw AMQP message body.
/// - `routing_key` — the routing key from the AMQP delivery.
/// - `acker` — used to ACK/NACK the delivery after processing.
pub type HandlerFn =
    Arc<dyn Fn(Vec<u8>, String, DeliveryAcker) -> BoxFuture<'static, ()> + Send + Sync>;

/// An inbound port: a source that delivers raw telemetry envelope bytes.
#[async_trait]
pub trait EnvelopeConsumer: Send + Sync {
    /// Start consuming messages.
    async fn start(&self, handler: HandlerFn) -> Result<(), XtceError>;
}

/// A no-op acker for use in unit tests.
pub struct NoOpAcker;

#[async_trait]
impl AckerInner for NoOpAcker {
    async fn ack(&mut self) {}
    async fn nack(&mut self) {}
}

impl DeliveryAcker {
    /// Construct a no-op acker for use in unit tests.
    pub fn noop() -> Self {
        Self {
            inner: Box::new(NoOpAcker),
        }
    }
}

// ── Inbound Port: EnvelopeConsumer ───────────────────────────────────────────
//
// This trait is the boundary between the application core and the message bus.
// The orchestrator never depends on lapin, RabbitMQ, or any AMQP type.
// It only knows about this trait.
//
// In hexagonal architecture terms: this is the DRIVING port — the adapter that
// pushes data INTO the application core.
//
// Concrete implementor (Sprint 2): `adapters::inbound::rabbitmq_consumer::RabbitMqConsumer`

use std::sync::Arc;

use async_trait::async_trait;
use futures::future::BoxFuture;

use crate::domain::errors::DecoderError;

/// A handle the consumer gives to the handler so it can ACK or NACK the
/// original AMQP delivery AFTER the handler has finished processing.
///
/// This decouples the acknowledgement mechanism from the AMQP types —
/// the orchestrator never touches a `lapin::Channel` directly.
pub struct DeliveryAcker {
    inner: Box<dyn AckerInner + Send>,
}

/// Private trait implemented by the RabbitMQ adapter.
/// Hidden from the application core.
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
///
/// The consumer calls this closure for each incoming AMQP delivery.
/// - `raw_bytes` — the raw AMQP message body (Protobuf bytes, not decoded).
/// - `routing_key` — the routing key from the AMQP delivery.
/// - `acker`     — used to ACK or NACK the delivery after processing.
pub type HandlerFn =
    Arc<dyn Fn(Vec<u8>, String, DeliveryAcker) -> BoxFuture<'static, ()> + Send + Sync>;

/// An inbound port: a source that delivers raw telemetry envelope bytes.
///
/// Implementors connect to the message bus, receive deliveries, and call
/// the `handler` once per message.
///
/// The handler is responsible for:
///   1. Deserializing the bytes into a `TelemetryEnvelope`.
///   2. Running the domain pipeline.
///   3. Calling `acker.ack()` on success or `acker.nack()` on permanent failure.
#[async_trait]
pub trait EnvelopeConsumer: Send + Sync {
    /// Start consuming messages.
    ///
    /// This call blocks (drives the message loop) until either the connection
    /// drops or an unrecoverable error occurs.
    ///
    /// Returns `Err(DecoderError::AmqpError(...))` on unrecoverable failure.
    async fn start(&self, handler: HandlerFn) -> Result<(), DecoderError>;
}

// ── AMQP-backed DeliveryAcker ─────────────────────────────────────────────────
//
// Concrete implementation lives in the inbound adapter, not here.
// The `pub(crate)` constructor is the only bridge.

/// A no-op acker for use in unit tests — does not require a live broker.
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

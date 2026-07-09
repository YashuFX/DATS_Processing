use std::sync::Arc;
use async_trait::async_trait;
use futures::future::BoxFuture;
use crate::domain::errors::DomainError;

pub struct DeliveryAcker {
    inner: Box<dyn AckerInner + Send>,
}

#[async_trait]
pub(crate) trait AckerInner {
    async fn ack(&mut self);
    async fn nack(&mut self);
}

impl DeliveryAcker {
    #[allow(dead_code)]
    pub(crate) fn new(inner: Box<dyn AckerInner + Send>) -> Self {
        Self { inner }
    }

    pub async fn ack(mut self) {
        self.inner.ack().await;
    }

    pub async fn nack(mut self) {
        self.inner.nack().await;
    }
}

pub type HandlerFn =
    Arc<dyn Fn(Vec<u8>, String, DeliveryAcker) -> BoxFuture<'static, ()> + Send + Sync>;

#[async_trait]
pub trait EnvelopeConsumer: Send + Sync {
    async fn start(&self, handler: HandlerFn) -> Result<(), DomainError>;
}

pub struct NoOpAcker;

#[async_trait]
impl AckerInner for NoOpAcker {
    async fn ack(&mut self) {}
    async fn nack(&mut self) {}
}

impl DeliveryAcker {
    pub fn noop() -> Self {
        Self {
            inner: Box::new(NoOpAcker),
        }
    }
}

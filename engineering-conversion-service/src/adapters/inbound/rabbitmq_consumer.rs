use async_trait::async_trait;
use futures::StreamExt;
use lapin::{
    options::{
        BasicAckOptions, BasicConsumeOptions, BasicNackOptions, QueueBindOptions,
        QueueDeclareOptions,
    },
    types::FieldTable,
    Channel, Connection, ConnectionProperties,
};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::domain::errors::DomainError;
use crate::ports::inbound::{AckerInner, DeliveryAcker, EnvelopeConsumer, HandlerFn};

struct LapinAcker {
    channel: Channel,
    delivery_tag: u64,
}

#[async_trait]
impl AckerInner for LapinAcker {
    async fn ack(&mut self) {
        if let Err(e) = self
            .channel
            .basic_ack(self.delivery_tag, BasicAckOptions::default())
            .await
        {
            tracing::error!("Failed to ACK delivery tag {}: {:?}", self.delivery_tag, e);
        }
    }

    async fn nack(&mut self) {
        let options = BasicNackOptions {
            multiple: false,
            requeue: false,
        };
        if let Err(e) = self.channel.basic_nack(self.delivery_tag, options).await {
            tracing::error!("Failed to NACK delivery tag {}: {:?}", self.delivery_tag, e);
        }
    }
}

pub struct RabbitMqConsumer {
    config: Arc<AppConfig>,
    conn: Arc<RwLock<Option<Connection>>>,
}

impl RabbitMqConsumer {
    pub fn new(config: Arc<AppConfig>) -> Self {
        Self {
            config,
            conn: Arc::new(RwLock::new(None)),
        }
    }

    /// Exposes whether the underlying RabbitMQ connection is active.
    pub async fn is_connected(&self) -> bool {
        if let Some(conn) = &*self.conn.read().await {
            conn.status().connected()
        } else {
            false
        }
    }
}

#[async_trait]
impl EnvelopeConsumer for RabbitMqConsumer {
    async fn start(&self, handler: HandlerFn) -> Result<(), DomainError> {
        tracing::info!("Connecting to RabbitMQ at {}", self.config.amqp_url);

        let conn = Connection::connect(&self.config.amqp_url, ConnectionProperties::default())
            .await
            .map_err(|e| DomainError::ConfigReadError("AMQP_URL".to_string(), format!("Failed to connect: {e}")))?;

        tracing::info!("RabbitMQ connection established. Creating channel...");
        let channel = conn
            .create_channel()
            .await
            .map_err(|e| DomainError::ConfigReadError("AMQP_URL".to_string(), format!("Failed to create channel: {e}")))?;

        // Store connection reference for health reporting
        {
            let mut conn_guard = self.conn.write().await;
            *conn_guard = Some(conn);
        }

        channel
            .basic_qos(
                self.config.prefetch_count,
                lapin::options::BasicQosOptions::default(),
            )
            .await
            .map_err(|e| DomainError::ConfigReadError("AMQP_URL".to_string(), format!("Failed to set basic QOS: {e}")))?;

        tracing::info!("Declaring queue: {}", self.config.source_queue);
        channel
            .queue_declare(
                &self.config.source_queue,
                QueueDeclareOptions {
                    durable: true,
                    ..QueueDeclareOptions::default()
                },
                FieldTable::default(),
            )
            .await
            .map_err(|e| {
                DomainError::ConfigReadError(
                    "SOURCE_QUEUE".to_string(),
                    format!("Failed to declare queue {}: {e}", self.config.source_queue),
                )
            })?;

        tracing::info!(
            "Binding queue {} to exchange {} with routing key {}",
            self.config.source_queue,
            self.config.source_exchange,
            self.config.source_routing_key
        );
        channel
            .queue_bind(
                &self.config.source_queue,
                &self.config.source_exchange,
                &self.config.source_routing_key,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await
            .map_err(|e| {
                DomainError::ConfigReadError(
                    "SOURCE_QUEUE".to_string(),
                    format!(
                        "Failed to bind queue {} to exchange {}: {e}",
                        self.config.source_queue, self.config.source_exchange
                    ),
                )
            })?;

        let mut consumer = channel
            .basic_consume(
                &self.config.source_queue,
                &self.config.consumer_tag,
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .map_err(|e| DomainError::ConfigReadError("AMQP_URL".to_string(), format!("Failed to start consumer: {e}")))?;

        tracing::info!("Consumer started successfully. Awaiting messages...");

        while let Some(delivery_result) = consumer.next().await {
            match delivery_result {
                Ok(delivery) => {
                    let raw_bytes = delivery.data;
                    let delivery_tag = delivery.delivery_tag;
                    let chan_clone = channel.clone();

                    let inner_acker = LapinAcker {
                        channel: chan_clone,
                        delivery_tag,
                    };
                    let acker = DeliveryAcker::new(Box::new(inner_acker));
                    let routing_key = delivery.routing_key.to_string();

                    let handler_clone = handler.clone();
                    tokio::spawn(async move {
                        handler_clone(raw_bytes, routing_key, acker).await;
                    });
                }
                Err(e) => {
                    tracing::error!("AMQP delivery error: {:?}", e);
                    return Err(DomainError::ConfigReadError(
                        "AMQP_URL".to_string(),
                        format!("Delivery stream error: {e}"),
                    ));
                }
            }
        }

        Ok(())
    }
}

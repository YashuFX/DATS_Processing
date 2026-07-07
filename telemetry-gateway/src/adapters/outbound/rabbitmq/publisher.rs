use crate::api::telemetry::v1::TelemetryEnvelope;
use crate::domain::errors::GatewayError;
use crate::ports::outbound::publish_port::PublishPort;

use lapin::{
    options::{BasicPublishOptions, ConfirmSelectOptions, ExchangeDeclareOptions},
    publisher_confirm::PublisherConfirm,
    types::FieldTable,
    BasicProperties, Channel, Connection, ConnectionProperties, ExchangeKind,
};
use prost::Message;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

const EXCHANGE_NAME: &str = "telemetry.raw";
const RECONNECT_DELAY_MS: u64 = 5_000;

#[allow(dead_code)]
pub struct RabbitMqPublisherAdapter {
    amqp_url:  String,
    channel:   Arc<Mutex<Option<Channel>>>,
    connected: Arc<AtomicBool>,
    pending:   Arc<AtomicUsize>,
}

impl RabbitMqPublisherAdapter {
    /// Create the adapter and immediately start the background connection loop.
    pub fn new(amqp_url: &str) -> Self {
        let amqp_url = amqp_url.to_string();
        let channel = Arc::new(Mutex::new(None::<Channel>));
        let connected = Arc::new(AtomicBool::new(false));
        let pending = Arc::new(AtomicUsize::new(0));

        let channel_clone = Arc::clone(&channel);
        let connected_clone = Arc::clone(&connected);
        let url_clone = amqp_url.clone();

        // Background reconnect loop
        tokio::spawn(async move {
            loop {
                info!("RabbitMQ: connecting to {}...", url_clone);

                match Connection::connect(&url_clone, ConnectionProperties::default()).await {
                    Ok(conn) => {
                        info!("RabbitMQ: connection established.");

                        match conn.create_channel().await {
                            Ok(ch) => {
                                // Enable publisher confirms on this channel
                                if let Err(e) = ch
                                    .confirm_select(ConfirmSelectOptions::default())
                                    .await
                                {
                                    error!("RabbitMQ: confirm_select failed: {:?}", e);
                                    tokio::time::sleep(
                                        std::time::Duration::from_millis(RECONNECT_DELAY_MS),
                                    )
                                    .await;
                                    continue;
                                }

                                // Declare the fanout exchange (idempotent)
                                if let Err(e) = ch
                                    .exchange_declare(
                                        EXCHANGE_NAME,
                                        ExchangeKind::Topic,
                                        ExchangeDeclareOptions {
                                            durable: true,
                                            ..Default::default()
                                        },
                                        FieldTable::default(),
                                    )
                                    .await
                                {
                                    error!("RabbitMQ: exchange_declare failed: {:?}", e);
                                    tokio::time::sleep(
                                        std::time::Duration::from_millis(RECONNECT_DELAY_MS),
                                    )
                                    .await;
                                    continue;
                                }

                                info!(
                                    "RabbitMQ: channel ready, exchange '{}' declared.",
                                    EXCHANGE_NAME
                                );
                                connected_clone.store(true, Ordering::SeqCst);
                                *channel_clone.lock().await = Some(ch.clone());

                                // Register error callback (non-async)
                                conn.on_error(|e| {
                                    error!("RabbitMQ: connection error: {:?}", e);
                                });

                                // Poll until the channel is no longer usable
                                loop {
                                    tokio::time::sleep(
                                        std::time::Duration::from_millis(500),
                                    )
                                    .await;
                                    if !ch.status().connected() {
                                        warn!("RabbitMQ: channel closed, will reconnect.");
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                error!("RabbitMQ: create_channel failed: {:?}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("RabbitMQ: connection failed: {:?}. Retrying in {}ms", e, RECONNECT_DELAY_MS);
                    }
                }

                connected_clone.store(false, Ordering::SeqCst);
                *channel_clone.lock().await = None;
                warn!("RabbitMQ: disconnected. Reconnecting in {}ms...", RECONNECT_DELAY_MS);
                tokio::time::sleep(std::time::Duration::from_millis(RECONNECT_DELAY_MS)).await;
            }
        });

        Self {
            amqp_url,
            channel,
            connected,
            pending,
        }
    }
}

#[tonic::async_trait]
impl PublishPort for RabbitMqPublisherAdapter {
    async fn publish(
        &self,
        envelope: TelemetryEnvelope,
        routing_key: &str,
    ) -> Result<(), GatewayError> {
        let guard = self.channel.lock().await;
        let ch = guard.as_ref().ok_or_else(|| {
            GatewayError::PublishError("RabbitMQ channel is not available".to_string())
        })?;

        // Serialize the envelope to protobuf bytes
        let payload = envelope.encode_to_vec();

        self.pending.fetch_add(1, Ordering::SeqCst);

        let confirm: PublisherConfirm = ch
            .basic_publish(
                EXCHANGE_NAME,
                routing_key,
                BasicPublishOptions::default(),
                &payload,
                BasicProperties::default()
                    .with_delivery_mode(2) // persistent
                    .with_content_type("application/x-protobuf".into()),
            )
            .await
            .map_err(|e| GatewayError::PublishError(format!("basic_publish error: {:?}", e)))?;

        self.pending.fetch_sub(1, Ordering::SeqCst);

        // Wait for broker ack (publisher confirm)
        confirm
            .await
            .map_err(|e| GatewayError::PublishError(format!("publisher confirm error: {:?}", e)))?;

        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn buffer_depth(&self) -> usize {
        self.pending.load(Ordering::SeqCst)
    }
}

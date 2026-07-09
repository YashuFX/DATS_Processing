// ── RabbitMQ Publisher Adapter ────────────────────────────────────────────────
//
// Responsibility: Implement the DecodedPublisher outbound port.
// Handles serializing mutated TelemetryEnvelopes, publishing them to the broker,
// awaiting publisher confirms, and handling transient network retries with
// exponential backoff.

use async_trait::async_trait;
use lapin::{Connection, ConnectionProperties};
use prost::Message;
use std::sync::Arc;
use std::time::Duration;

use crate::config::AppConfig;
use crate::domain::errors::DecoderError;
use crate::ports::outbound::DecodedPublisher;
use crate::proto::TelemetryEnvelope;

pub struct RabbitMqPublisher {
    config: Arc<AppConfig>,
    state: tokio::sync::Mutex<PublisherState>,
}

struct PublisherState {
    _connection: Connection,
    channel: lapin::Channel,
}

impl RabbitMqPublisher {
    /// Create a new RabbitMqPublisher and establish connection & channel.
    pub async fn new(config: Arc<AppConfig>) -> Result<Self, DecoderError> {
        let state = Self::connect(&config).await?;
        Ok(Self {
            config,
            state: tokio::sync::Mutex::new(state),
        })
    }

    /// Helper to connect and setup confirms.
    async fn connect(config: &AppConfig) -> Result<PublisherState, DecoderError> {
        tracing::info!("Publisher connecting to RabbitMQ at {}...", config.amqp_url);
        let connection = Connection::connect(&config.amqp_url, ConnectionProperties::default())
            .await
            .map_err(|e| DecoderError::AmqpError(format!("Publisher connection failed: {e}")))?;

        let channel = connection.create_channel().await.map_err(|e| {
            DecoderError::AmqpError(format!("Publisher channel creation failed: {e}"))
        })?;

        // Enable publisher confirms
        channel
            .confirm_select(lapin::options::ConfirmSelectOptions::default())
            .await
            .map_err(|e| {
                DecoderError::AmqpError(format!("Publisher confirm_select failed: {e}"))
            })?;

        // Declare the destination exchange as a durable topic exchange
        channel
            .exchange_declare(
                &config.destination_exchange,
                lapin::ExchangeKind::Topic,
                lapin::options::ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
            .map_err(|e| {
                DecoderError::AmqpError(format!("Publisher exchange_declare failed: {e}"))
            })?;

        Ok(PublisherState {
            _connection: connection,
            channel,
        })
    }
}

#[async_trait]
impl DecodedPublisher for RabbitMqPublisher {
    async fn publish(
        &self,
        envelope: &TelemetryEnvelope,
        routing_key: &str,
    ) -> Result<(), DecoderError> {
        let mut bytes = Vec::new();
        envelope
            .encode(&mut bytes)
            .map_err(|e| DecoderError::ProtoDecodeError(e.to_string()))?;

        let mut attempts = 0;
        let mut delay = Duration::from_millis(100);
        let max_attempts = self.config.retry_max_attempts;

        loop {
            attempts += 1;

            // Lock the publisher state to get the channel
            let mut state_guard = self.state.lock().await;

            let publish_options = lapin::options::BasicPublishOptions::default();
            let properties = lapin::BasicProperties::default();

            tracing::debug!(
                "Publishing envelope ID={} to exchange='{}' with routing_key='{}' (attempt {}/{})",
                envelope.envelope_id,
                self.config.destination_exchange,
                routing_key,
                attempts,
                max_attempts
            );

            let publish_res = state_guard
                .channel
                .basic_publish(
                    &self.config.destination_exchange,
                    routing_key,
                    publish_options,
                    &bytes,
                    properties,
                )
                .await;

            match publish_res {
                Ok(confirm) => {
                    // Wait for publisher confirm (timeout-bounded)
                    let timeout_duration = Duration::from_millis(self.config.publish_timeout_ms);
                    let confirm_future = confirm;

                    match tokio::time::timeout(timeout_duration, confirm_future).await {
                        Ok(Ok(_)) => {
                            // Successfully published and confirmed!
                            return Ok(());
                        }
                        Ok(Err(e)) => {
                            tracing::warn!(
                                "Publisher confirm returned NACK/error on attempt {}: {:?}",
                                attempts,
                                e
                            );
                        }
                        Err(_) => {
                            tracing::warn!(
                                "Publisher confirm timed out after {}ms on attempt {}",
                                self.config.publish_timeout_ms,
                                attempts
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Basic publish call failed on attempt {}: {:?}", attempts, e);

                    // Connection/channel is broken; attempt to reconnect before the next retry
                    tracing::info!("Publisher channel broken. Reconnecting...");
                    match Self::connect(&self.config).await {
                        Ok(new_state) => {
                            *state_guard = new_state;
                            tracing::info!("Publisher reconnected successfully.");
                        }
                        Err(conn_err) => {
                            tracing::error!("Publisher reconnect failed: {:?}", conn_err);
                        }
                    }
                }
            }

            // If we have exhausted all attempts, return an error
            if attempts >= max_attempts {
                return Err(DecoderError::AmqpError(format!(
                    "Failed to publish and confirm envelope {} after {} attempts",
                    envelope.envelope_id, max_attempts
                )));
            }

            // Pseudo-random jitter based on current time elapsed nanos (prevents thundering herd)
            let jitter_ms = (tokio::time::Instant::now().elapsed().as_nanos() % 50) as u64;
            let sleep_duration = delay + Duration::from_millis(jitter_ms);

            tracing::info!("Retrying publish in {:?}", sleep_duration);
            tokio::time::sleep(sleep_duration).await;

            // Double the backoff duration (exponential backoff)
            delay *= 2;
        }
    }
}

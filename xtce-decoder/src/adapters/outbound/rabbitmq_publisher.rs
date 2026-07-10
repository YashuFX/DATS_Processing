use async_trait::async_trait;
use lapin::{Connection, ConnectionProperties};
use prost::Message;
use std::sync::Arc;
use std::time::Duration;

use crate::config::AppConfig;
use crate::domain::errors::XtceError;
use crate::ports::outbound::EngineeringPublisher;
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
    pub async fn new(config: Arc<AppConfig>) -> Result<Self, XtceError> {
        let state = Self::connect(&config).await?;
        Ok(Self {
            config,
            state: tokio::sync::Mutex::new(state),
        })
    }

    /// Helper to connect and setup confirms.
    async fn connect(config: &AppConfig) -> Result<PublisherState, XtceError> {
        tracing::info!("Publisher connecting to RabbitMQ at {}...", config.amqp_url);
        let connection = Connection::connect(&config.amqp_url, ConnectionProperties::default())
            .await
            .map_err(|e| XtceError::ConfigError(format!("Publisher connection failed: {e}")))?;

        let channel = connection.create_channel().await.map_err(|e| {
            XtceError::ConfigError(format!("Publisher channel creation failed: {e}"))
        })?;

        // Enable publisher confirms
        channel
            .confirm_select(lapin::options::ConfirmSelectOptions::default())
            .await
            .map_err(|e| {
                XtceError::ConfigError(format!("Publisher confirm_select failed: {e}"))
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
                XtceError::ConfigError(format!("Publisher exchange_declare failed: {e}"))
            })?;

        Ok(PublisherState {
            _connection: connection,
            channel,
        })
    }
}

#[async_trait]
impl EngineeringPublisher for RabbitMqPublisher {
    async fn publish(&self, envelope: &TelemetryEnvelope, routing_key: &str) -> Result<(), XtceError> {
        let mut bytes = Vec::new();
        envelope
            .encode(&mut bytes)
            .map_err(|e| XtceError::DecommutationFailed(format!("Failed to encode envelope: {e}")))?;

        let mut attempts = 0;
        let mut delay = Duration::from_millis(100);
        let max_attempts = self.config.retry_max_attempts;

        loop {
            attempts += 1;

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
                    let timeout_duration = Duration::from_millis(self.config.publish_timeout_ms);
                    match tokio::time::timeout(timeout_duration, confirm).await {
                        Ok(Ok(_)) => {
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

            if attempts >= max_attempts {
                return Err(XtceError::DecommutationFailed(format!(
                    "Failed to publish and confirm envelope {} after {} attempts",
                    envelope.envelope_id, max_attempts
                )));
            }

            let jitter_ms = (tokio::time::Instant::now().elapsed().as_nanos() % 50) as u64;
            let sleep_duration = delay + Duration::from_millis(jitter_ms);

            tracing::info!("Retrying publish in {:?}", sleep_duration);
            tokio::time::sleep(sleep_duration).await;

            delay *= 2;
        }
    }
}

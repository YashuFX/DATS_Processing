// ── CCSDS Decoder Service — Sprint 3 Entry Point ─────────────────────────────
//
// In hexagonal architecture, this is the Composition Root.
// It is responsible for loading configuration, constructing the concrete
// adapters, plugging them into the port traits, and starting the async consumer.

pub mod adapters;
pub mod application;
pub mod config;
pub mod domain;
pub mod ports;
pub mod proto;

use futures::future::FutureExt;
use std::sync::Arc;

use crate::adapters::inbound::rabbitmq_consumer::RabbitMqConsumer;
use crate::adapters::outbound::console_sink::ConsoleSink;
use crate::adapters::outbound::rabbitmq_publisher::RabbitMqPublisher;
use crate::application::orchestrator::DecoderOrchestrator;
use crate::config::AppConfig;
use crate::ports::inbound::EnvelopeConsumer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("CCSDS Decoder Service — Sprint 3 Starting...");

    // 2. Load configuration from environment (fail-fast if invalid)
    let config = match AppConfig::from_env() {
        Ok(cfg) => Arc::new(cfg),
        Err(e) => {
            tracing::error!("Configuration failure: {}", e);
            std::process::exit(1);
        }
    };

    tracing::info!(
        "Config loaded successfully. Prefetch={}, CheckCRC={}",
        config.prefetch_count,
        config.check_crc
    );

    // 3. Construct outbound adapters
    let sink = Arc::new(ConsoleSink::new());

    let publisher = match RabbitMqPublisher::new(config.clone()).await {
        Ok(publ) => Arc::new(publ),
        Err(e) => {
            tracing::error!("Failed to initialize RabbitMQ publisher: {}", e);
            std::process::exit(1);
        }
    };

    // 4. Construct orchestrator (application core)
    let orchestrator = Arc::new(DecoderOrchestrator::new(sink, publisher, config.check_crc));

    // 5. Construct inbound adapter
    let consumer = RabbitMqConsumer::new(config.clone());

    // 6. Define the handler closure
    let handler = {
        let orch = orchestrator.clone();
        Arc::new(
            move |raw_bytes: Vec<u8>,
                  routing_key: String,
                  acker: crate::ports::inbound::DeliveryAcker| {
                let orch = orch.clone();
                async move {
                    match orch.on_envelope_consumed(raw_bytes, &routing_key).await {
                        Ok(()) => {
                            acker.ack().await;
                        }
                        Err(e) => {
                            tracing::error!("Processing failed: {}", e);
                            // Under Hexagonal constraints, we discard/NACK poison messages
                            // to avoid blocking the queue. In production, dead letter queues
                            // handle this.
                            acker.nack().await;
                        }
                    }
                }
                .boxed() // boxes the future into BoxFuture
            },
        )
    };

    // 7. Start the consumer loop (this blocks until connection failure or SIGINT)
    tracing::info!("Starting AMQP consume loop...");
    if let Err(e) = consumer.start(handler).await {
        tracing::error!("AMQP Consumer stopped with error: {:?}", e);
        std::process::exit(1);
    }

    Ok(())
}

pub mod proto;
pub mod domain;
pub mod config;
pub mod ports;
pub mod adapters;
pub mod orchestrator;

use std::sync::Arc;
use crate::config::AppConfig;
use crate::domain::registry::XtceRegistry;
use crate::adapters::inbound::RabbitMqConsumer;
use crate::adapters::outbound::{RabbitMqPublisher, ConsoleAlertPort};
use crate::orchestrator::DecoderOrchestrator;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize tracing log subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("XTCE Decoder Service — Starting...");

    // 2. Load environment configuration
    let config = match AppConfig::from_env() {
        Ok(cfg) => Arc::new(cfg),
        Err(e) => {
            tracing::error!("Configuration loading failed: {}", e);
            std::process::exit(1);
        }
    };

    tracing::info!(
        "Configuration successfully loaded. Exchange={}, Queue={}, DbDir={}",
        config.source_exchange,
        config.source_queue,
        config.xtce_db_dir
    );

    // 3. Construct XTCE database compiler & cache registry
    let registry = Arc::new(XtceRegistry::new(config.xtce_db_dir.clone()));

    // 4. Construct outbound adapters (publisher and alert sink)
    let alert_port = Arc::new(ConsoleAlertPort);

    let publisher = match RabbitMqPublisher::new(config.clone()).await {
        Ok(publ) => Arc::new(publ),
        Err(e) => {
            tracing::error!("Failed to initialize RabbitMQ publisher: {:?}", e);
            std::process::exit(1);
        }
    };

    // 5. Construct inbound consumer adapter
    let consumer = Arc::new(RabbitMqConsumer::new(config.clone()));

    // 6. Assemble the Orchestrator
    let orchestrator = DecoderOrchestrator::new(
        config,
        registry,
        consumer,
        publisher,
        alert_port,
    );

    // 7. Start the consume-decommute loop
    tracing::info!("Starting AMQP consumer loop...");
    if let Err(e) = orchestrator.start().await {
        tracing::error!("Orchestrator stopped due to error: {:?}", e);
        std::process::exit(1);
    }

    Ok(())
}

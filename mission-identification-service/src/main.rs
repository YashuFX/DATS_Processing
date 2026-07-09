pub mod domain;
pub mod application;
pub mod ports;
pub mod config;
pub mod proto;
pub mod adapters;

use std::fs;
use std::sync::Arc;
use futures::future::FutureExt;

use crate::config::AppConfig;
use crate::domain::registry::MissionRegistry;
use crate::domain::lookup::RuleLookupEngine;
use crate::adapters::inbound::rabbitmq_consumer::RabbitMqConsumer;
use crate::adapters::outbound::rabbitmq_publisher::RabbitMqPublisher;
use crate::adapters::outbound::logging_alert::LoggingAlertAdapter;
use crate::application::orchestrator::IdentificationOrchestrator;
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

    tracing::info!("Mission Identification Service — Starting...");

    // 2. Load configuration from environment
    let config = match AppConfig::from_env() {
        Ok(cfg) => Arc::new(cfg),
        Err(e) => {
            tracing::error!("Configuration failure: {}", e);
            std::process::exit(1);
        }
    };

    // 3. Initialize Prometheus Metrics Exporter HTTP Endpoint
    let metrics_addr = format!("0.0.0.0:{}", config.metrics_port)
        .parse::<std::net::SocketAddr>()?;
    if let Err(e) = metrics_exporter_prometheus::PrometheusBuilder::new()
        .with_http_listener(metrics_addr)
        .install()
    {
        tracing::error!("Failed to initialize Prometheus exporter: {}", e);
        std::process::exit(1);
    }
    tracing::info!("Prometheus metrics scraping endpoint active at http://{}", metrics_addr);

    // 4. Load & Validate Registry YAML file
    tracing::info!("Loading registry file: {}", config.registry_file_path);
    let yaml_content = match fs::read_to_string(&config.registry_file_path) {
        Ok(content) => content,
        Err(e) => {
            tracing::error!("Failed to read registry file: {}", e);
            std::process::exit(1);
        }
    };

    let registry = match MissionRegistry::from_yaml(&yaml_content) {
        Ok(reg) => reg,
        Err(e) => {
            tracing::error!("Registry syntax validation failed: {}", e);
            std::process::exit(1);
        }
    };

    let mission_count = registry.missions().len();
    let satellite_count: usize = registry.missions().iter().map(|m| m.satellites.len()).sum();
    tracing::info!(
        "Registry loaded successfully: {} missions, {} satellites.",
        mission_count,
        satellite_count
    );

    // 5. Instantiate Domain Lookup Engine
    let lookup_engine = Arc::new(RuleLookupEngine::new(registry));

    // 6. Instantiate Outbound Adapters
    let publisher = match RabbitMqPublisher::new(config.clone()).await {
        Ok(publ) => Arc::new(publ),
        Err(e) => {
            tracing::error!("Failed to initialize RabbitMQ publisher: {}", e);
            std::process::exit(1);
        }
    };

    let alert_port = Arc::new(LoggingAlertAdapter::new());

    // 7. Construct Application Orchestrator
    let orchestrator = Arc::new(IdentificationOrchestrator::new(
        lookup_engine,
        publisher,
        alert_port,
    ));

    // 8. Construct Inbound Adapter & Handler Closure
    let consumer = RabbitMqConsumer::new(config.clone());
    let handler = {
        let orch = orchestrator.clone();
        Arc::new(
            move |raw_bytes: Vec<u8>,
                  routing_key: String,
                  acker: crate::ports::inbound::DeliveryAcker| {
                let orch = orch.clone();
                async move {
                    orch.on_envelope_consumed(raw_bytes, routing_key, acker).await;
                }
                .boxed()
            },
        )
    };

    // 9. Start driving consume loop
    tracing::info!("Starting AMQP consume loop...");
    if let Err(e) = consumer.start(handler).await {
        tracing::error!("AMQP Consumer stopped with error: {:?}", e);
        std::process::exit(1);
    }

    Ok(())
}

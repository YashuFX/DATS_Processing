pub mod domain;
pub mod application;
pub mod ports;
pub mod config;
pub mod proto;
pub mod adapters;

use std::path::PathBuf;
use std::sync::Arc;
use futures::future::FutureExt;

use crate::config::AppConfig;
use crate::domain::registry::FormulaRegistry;
use crate::adapters::inbound::rabbitmq_consumer::RabbitMqConsumer;
use crate::adapters::outbound::rabbitmq_publisher::RabbitMqPublisher;
use crate::adapters::outbound::console_sink::ConsoleAlertPort;
use crate::application::orchestrator::ConversionOrchestrator;
use crate::ports::inbound::EnvelopeConsumer;

/// Simple HTTP server to handle liveness/readiness probes.
async fn run_health_check_server(port: u16, consumer: Arc<RabbitMqConsumer>) {
    let addr = format!("0.0.0.0:{}", port);
    if let Ok(listener) = tokio::net::TcpListener::bind(&addr).await {
        tracing::info!("Health check server active at http://{}/health", addr);
        loop {
            if let Ok((mut socket, _)) = listener.accept().await {
                let consumer_clone = consumer.clone();
                tokio::spawn(async move {
                    let mut buf = [0; 1024];
                    let _ = socket.try_read(&mut buf); // Consume request headers
                    let is_connected = consumer_clone.is_connected().await;
                    let response = if is_connected {
                        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK"
                    } else {
                        "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain\r\nContent-Length: 11\r\nConnection: close\r\n\r\nUNHEALTHY"
                    };
                    let _ = tokio::io::AsyncWriteExt::write_all(&mut socket, response.as_bytes()).await;
                });
            }
        }
    } else {
        tracing::error!("Failed to bind health check server to {}", addr);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Engineering Conversion Service — Starting...");

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

    // 4. Instantiate Formula Registry
    let registry_path = PathBuf::from(&config.derived_db_dir);
    tracing::info!("Initializing formula registry with directory: {:?}", registry_path);
    let registry = Arc::new(FormulaRegistry::new(registry_path));

    // 5. Instantiate Outbound Adapters
    let publisher = match RabbitMqPublisher::new(config.clone()).await {
        Ok(publ) => Arc::new(publ),
        Err(e) => {
            tracing::error!("Failed to initialize RabbitMQ publisher: {}", e);
            std::process::exit(1);
        }
    };

    let alert_port = Arc::new(ConsoleAlertPort);

    // 6. Construct Application Orchestrator
    let orchestrator = Arc::new(ConversionOrchestrator::new(
        config.clone(),
        registry,
        publisher,
        alert_port,
    ));

    // 7. Construct Inbound Adapter & Handler Closure
    let consumer = Arc::new(RabbitMqConsumer::new(config.clone()));
    let handler = {
        let orch = orchestrator.clone();
        Arc::new(
            move |raw_bytes: Vec<u8>,
                  routing_key: String,
                  acker: crate::ports::inbound::DeliveryAcker| {
                let orch = orch.clone();
                async move {
                    orch.handle_delivery(raw_bytes, routing_key, acker).await;
                }
                .boxed()
            },
        )
    };

    // Spawn health check server
    let health_port = config.health_port;
    let health_consumer = consumer.clone();
    tokio::spawn(async move {
        run_health_check_server(health_port, health_consumer).await;
    });

    // 8. Start driving consume loop with graceful shutdown select
    tracing::info!("Starting AMQP consume loop...");
    let runner_consumer = consumer.clone();
    tokio::select! {
        res = runner_consumer.start(handler) => {
            if let Err(e) = res {
                tracing::error!("AMQP Consumer stopped with error: {:?}", e);
                std::process::exit(1);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received Ctrl-C signal. Shutting down gracefully...");
        }
    }

    tracing::info!("Engineering Conversion Service stopped.");
    Ok(())
}

pub mod api;
pub mod domain;
pub mod ports;
pub mod application;
pub mod adapters;

use std::net::SocketAddr;
use std::sync::Arc;

use tonic::transport::Server;

use crate::api::gateway::v1::telemetry_ingress_service_server::TelemetryIngressServiceServer;
use crate::adapters::inbound::grpc::replay_receiver::TelemetryIngressServiceAdapter;
use crate::adapters::outbound::rabbitmq::publisher::RabbitMqPublisherAdapter;
use crate::application::orchestrator::IngestionOrchestrator;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── 1. Logging ──────────────────────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Initializing Telemetry Gateway Service (Sprint 3)...");

    // ── 2. RabbitMQ publisher adapter ───────────────────────────────────────
    //       Change AMQP URL via environment variable AMQP_URL if needed.
    let amqp_url = std::env::var("AMQP_URL")
        .unwrap_or_else(|_| "amqp://guest:guest@127.0.0.1:5672/%2f".to_string());

    tracing::info!("RabbitMQ target: {}", amqp_url);
    let publish_port = Arc::new(RabbitMqPublisherAdapter::new(&amqp_url));

    // ── 3. Ingestion orchestrator ────────────────────────────────────────────
    let orchestrator = Arc::new(IngestionOrchestrator::new(publish_port));

    // ── 4. gRPC adapter ─────────────────────────────────────────────────────
    let grpc_adapter = TelemetryIngressServiceAdapter::new(orchestrator);

    // ── 5. Bind and serve ────────────────────────────────────────────────────
    let addr: SocketAddr = "0.0.0.0:50052".parse()?;
    tracing::info!("gRPC Ingress Server listening on {}", addr);

    Server::builder()
        .add_service(TelemetryIngressServiceServer::new(grpc_adapter))
        .serve(addr)
        .await?;

    Ok(())
}

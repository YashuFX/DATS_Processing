mod domain;
mod config;
mod api;
mod ports;
mod adapters;
mod application;

use std::sync::Arc;
use std::net::SocketAddr;
use tokio::sync::Mutex;
use config::Settings;
use tracing::{info, error, Level};
use tracing_subscriber::FmtSubscriber;
use crate::api::replay::v1::replay_service_server::ReplayServiceServer;
use crate::adapters::inbound::grpc_api::ReplayGrpcAdapter;
use crate::adapters::inbound::rest_api::create_rest_router;
use crate::adapters::outbound::file_reader::FileReaderAdapter;
use crate::adapters::outbound::grpc_publisher::GrpcPublisherAdapter;
use crate::adapters::outbound::event_publisher::LoggingEventPublisher;
use crate::adapters::outbound::metrics_exporter::PrometheusMetricsExporter;
use crate::application::orchestrator::ReplayOrchestrator;
use crate::application::command_handler::CommandHandler;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize Logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    info!("Initializing MuST Replay Simulator Service...");

    // 2. Load Configuration
    let config_path = "configs/default.yaml";
    let settings = Settings::load_from_file(config_path)?;
    info!("Successfully loaded configuration from: {}", config_path);

    // 3. Initialize Prometheus Metrics Exporter HTTP Endpoint
    let metrics_addr: SocketAddr = format!("0.0.0.0:{}", settings.observability.metrics_port).parse()?;
    metrics_exporter_prometheus::PrometheusBuilder::new()
        .with_http_listener(metrics_addr)
        .install()
        .expect("failed to install Prometheus recorder");
    info!("Prometheus metrics scraping endpoint active at http://{}", metrics_addr);

    // 4. Initialize Outbound (Driven) Adapters
    // Start with "binary" as default FileReader file type; dynamically updated on LoadFile command.
    let source_port = Arc::new(Mutex::new(FileReaderAdapter::new("binary")));

    let publish_port = Arc::new(GrpcPublisherAdapter::new(
        &settings.publisher.downstream_address,
        settings.publisher.buffer_size,
        settings.publisher.retry_attempts,
        settings.publisher.retry_delay_ms,
    ));

    let event_port = Arc::new(LoggingEventPublisher::new());
    let metrics_port = Arc::new(PrometheusMetricsExporter::new());

    // 5. Initialize Orchestrator and Command Handler
    let mission = settings.mission.as_ref().map(|m| crate::api::common::v1::MissionIdentifier {
        mission_id: m.mission_id,
        mission_name: m.mission_name.clone(),
        mission_code: m.mission_code.clone(),
    });

    let satellite = settings.satellite.as_ref().map(|s| crate::api::common::v1::SatelliteIdentifier {
        satellite_id: s.satellite_id,
        satellite_name: s.satellite_name.clone(),
        norad_id: s.norad_id,
    });

    let station = settings.station.as_ref().map(|st| crate::api::common::v1::GroundStationIdentifier {
        station_id: st.station_id,
        station_name: st.station_name.clone(),
        station_code: st.station_code.clone(),
    });

    let orchestrator = Arc::new(Mutex::new(ReplayOrchestrator::new(
        source_port,
        publish_port,
        metrics_port,
        event_port,
        mission,
        satellite,
        station,
    )));

    let command_handler = Arc::new(CommandHandler::new(orchestrator));

    // 6. Setup Inbound Servers (REST and gRPC)
    let rest_addr_str = format!("{}:{}", settings.server.rest.host, settings.server.rest.port);
    let rest_addr: SocketAddr = rest_addr_str.parse()?;
    let rest_router = create_rest_router(Arc::clone(&command_handler));
    let rest_listener = tokio::net::TcpListener::bind(&rest_addr).await?;
    let rest_server = axum::serve(rest_listener, rest_router);

    let grpc_addr_str = format!("{}:{}", settings.server.grpc.host, settings.server.grpc.port);
    let grpc_addr: SocketAddr = grpc_addr_str.parse()?;
    let grpc_service = ReplayGrpcAdapter::new(Arc::clone(&command_handler));
    let grpc_server = tonic::transport::Server::builder()
        .add_service(ReplayServiceServer::new(grpc_service))
        .serve(grpc_addr);

    // 7. Spawn Servers Concurrently
    let rest_handle = tokio::spawn(async move {
        info!("Starting REST API server on http://{}...", rest_addr);
        if let Err(e) = rest_server.await {
            error!("REST server stopped with error: {}", e);
        }
    });

    let grpc_handle = tokio::spawn(async move {
        info!("Starting gRPC API server on {}...", grpc_addr);
        if let Err(e) = grpc_server.await {
            error!("gRPC server stopped with error: {}", e);
        }
    });

    // Block on server processes
    let _ = tokio::join!(rest_handle, grpc_handle);

    Ok(())
}

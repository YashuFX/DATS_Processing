use crate::api::gateway::v1::telemetry_ingress_service_server::TelemetryIngressService;
use crate::api::gateway::v1::{TelemetryStreamRequest, TelemetryStreamResponse};
use crate::ports::inbound::ingest_port::IngestPort;

use std::sync::Arc;
use tonic::{Request, Response, Status, Streaming};

pub struct TelemetryIngressServiceAdapter {
    ingest_port: Arc<dyn IngestPort>,
}

impl TelemetryIngressServiceAdapter {
    pub fn new(ingest_port: Arc<dyn IngestPort>) -> Self {
        Self { ingest_port }
    }
}

#[tonic::async_trait]
impl TelemetryIngressService for TelemetryIngressServiceAdapter {
    async fn stream_telemetry(
        &self,
        request: Request<Streaming<TelemetryStreamRequest>>,
    ) -> Result<Response<TelemetryStreamResponse>, Status> {
        let mut stream = request.into_inner();
        let mut packets_processed = 0;
        let mut packets_rejected = 0;
        let mut session_id = String::new();
        let mut source_id = String::new();
        let mut connected_notified = false;

        while let Some(req_result) = stream.message().await.map_err(|e| {
            Status::internal(format!("Failed to read stream message: {:?}", e))
        })? {
            // Notify source connection on the first message
            if !connected_notified {
                source_id = req_result.source_id.clone();
                session_id = req_result.session_id.clone();
                if let Err(e) = self.ingest_port.on_source_connected(&source_id).await {
                    return Err(Status::internal(format!("Failed to handle source connection: {:?}", e)));
                }
                connected_notified = true;
            }

            if let Some(envelope) = req_result.envelope {
                match self.ingest_port.on_packet_received(envelope).await {
                    Ok(_) => {
                        packets_processed += 1;
                    }
                    Err(e) => {
                        packets_rejected += 1;
                        tracing::warn!("Packet ingestion rejected: {:?}", e);
                    }
                }
            } else {
                packets_rejected += 1;
            }
        }

        // Notify source disconnection / Session EOF
        if connected_notified {
            if let Err(e) = self.ingest_port.on_session_eof(&session_id).await {
                tracing::error!("Failed to notify session EOF: {:?}", e);
            }
            if let Err(e) = self.ingest_port.on_source_disconnected(&source_id).await {
                tracing::error!("Failed to notify source disconnect: {:?}", e);
            }
        }

        let reply = TelemetryStreamResponse {
            session_id,
            packets_processed,
            packets_rejected,
            is_healthy: true,
        };

        Ok(Response::new(reply))
    }
}

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex as StdMutex;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use futures_util::stream::Stream;
use tokio::sync::mpsc;
use crate::ports::{PublishPort, BackpressureStatus};
use crate::domain::errors::ReplayError;
use crate::api::telemetry::v1::TelemetryEnvelope;
use crate::api::gateway::v1::telemetry_ingress_service_client::TelemetryIngressServiceClient;
use crate::api::gateway::v1::TelemetryStreamRequest;

struct IngressStream {
    rx: Arc<StdMutex<mpsc::Receiver<TelemetryStreamRequest>>>,
}

impl Stream for IngressStream {
    type Item = TelemetryStreamRequest;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut guard = match self.rx.lock() {
            Ok(g) => g,
            Err(_) => return Poll::Ready(None),
        };
        guard.poll_recv(cx)
    }
}

pub struct GrpcPublisherAdapter {
    tx: mpsc::Sender<TelemetryStreamRequest>,
    connected: Arc<AtomicBool>,
    buffer_size: usize,
    session_id: String,
}

impl GrpcPublisherAdapter {
    /// Creates a new GrpcPublisherAdapter and spawns its resilient background connection task.
    pub fn new(
        downstream_address: &str,
        buffer_size: usize,
        _retry_attempts: u32,
        retry_delay_ms: u64,
    ) -> Self {
        let (tx, rx) = mpsc::channel(buffer_size);
        let rx_shared = Arc::new(StdMutex::new(rx));
        let connected = Arc::new(AtomicBool::new(false));
        let session_id = uuid::Uuid::new_v4().to_string();

        let connected_clone = Arc::clone(&connected);
        let address = downstream_address.to_string();

        tokio::spawn(async move {
            let retry_delay = Duration::from_millis(retry_delay_ms);

            loop {
                tracing::info!("Connecting to telemetry gateway at {}...", address);

                let mut client = match TelemetryIngressServiceClient::connect(address.clone()).await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("Connection to telemetry gateway failed: {}. Retrying...", e);
                        tokio::time::sleep(retry_delay).await;
                        continue;
                    }
                };

                tracing::info!("Telemetry gateway connected successfully!");
                connected_clone.store(true, Ordering::SeqCst);

                // Construct stream wrapping the persistent channel receiver
                let req_stream = IngressStream { rx: Arc::clone(&rx_shared) };

                // Call StreamTelemetry
                let request = tonic::Request::new(req_stream);
                match client.stream_telemetry(request).await {
                    Ok(response) => {
                        let resp = response.into_inner();
                        tracing::info!("gRPC ingestion stream completed cleanly. Response: {:?}", resp);
                    }
                    Err(status) => {
                        tracing::error!("gRPC ingestion stream dropped with error: {:?}", status);
                    }
                }

                connected_clone.store(false, Ordering::SeqCst);
                tracing::warn!("gRPC client disconnected. Reconnecting in {:?}", retry_delay);
                tokio::time::sleep(retry_delay).await;
            }
        });

        Self {
            tx,
            connected,
            buffer_size,
            session_id,
        }
    }
}

impl PublishPort for GrpcPublisherAdapter {
    fn publish(&self, envelope: TelemetryEnvelope) -> Result<(), ReplayError> {
        let req = TelemetryStreamRequest {
            source_id: "rss-replay".to_string(),
            session_id: self.session_id.clone(),
            envelope: Some(envelope),
        };

        // Try sending to the channel, if it fails because channel is full, we return a ReplayError.
        self.tx.try_send(req).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => {
                ReplayError::Network("Downstream publish buffer is full".to_string())
            }
            mpsc::error::TrySendError::Closed(_) => {
                ReplayError::FileIo("Downstream publish background task has exited".to_string())
            }
        })
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn backpressure_status(&self) -> BackpressureStatus {
        let cap = self.tx.capacity();
        // If remaining capacity is less than 10% of total buffer size, report HighWatermark
        if cap < (self.buffer_size / 10) {
            BackpressureStatus::HighWatermark
        } else {
            BackpressureStatus::Normal
        }
    }
}

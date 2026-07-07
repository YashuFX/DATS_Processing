use crate::api::telemetry::v1::TelemetryEnvelope;
use crate::domain::errors::GatewayError;
use crate::domain::validator::Validator;
use crate::domain::normalizer::Normalizer;
use crate::domain::enricher::Enricher;
use crate::domain::router::Router;
use crate::domain::models::{SourceRegistration, SourceStatus};
use crate::ports::inbound::ingest_port::IngestPort;
use crate::ports::outbound::publish_port::PublishPort;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct SessionStats {
    pub received:   AtomicU64,
    pub published:  AtomicU64,
    pub dropped:    AtomicU64,
    pub latencies_ms: Mutex<Vec<f64>>,
    pub delays_ms:    Mutex<Vec<f64>>,
}

impl SessionStats {
    pub fn new() -> Self {
        Self {
            received:     AtomicU64::new(0),
            published:    AtomicU64::new(0),
            dropped:      AtomicU64::new(0),
            latencies_ms: Mutex::new(Vec::new()),
            delays_ms:    Mutex::new(Vec::new()),
        }
    }
}

pub struct IngestionOrchestrator {
    seq_counter:      AtomicU64,
    stats:            Arc<SessionStats>,
    last_packet_time: Arc<Mutex<Option<Instant>>>,
    report_printed:   Arc<AtomicBool>,
    publish_port:     Arc<dyn PublishPort>,
}

impl IngestionOrchestrator {
    pub fn new(publish_port: Arc<dyn PublishPort>) -> Self {
        let stats            = Arc::new(SessionStats::new());
        let last_packet_time = Arc::new(Mutex::new(None::<Instant>));
        let report_printed   = Arc::new(AtomicBool::new(false));

        // Background monitor: print report 1 s after last packet
        let stats_c   = Arc::clone(&stats);
        let lpt_c     = Arc::clone(&last_packet_time);
        let printed_c = Arc::clone(&report_printed);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(200)).await;
                let print_now = {
                    let guard = lpt_c.lock().unwrap();
                    match *guard {
                        Some(t) if t.elapsed() >= Duration::from_secs(1) => {
                            !printed_c.load(Ordering::SeqCst)
                        }
                        _ => false,
                    }
                };
                if print_now {
                    printed_c.store(true, Ordering::SeqCst);
                    Self::print_report_static(&stats_c);
                }
            }
        });

        Self {
            seq_counter: AtomicU64::new(1),
            stats,
            last_packet_time,
            report_printed,
            publish_port,
        }
    }

    // ── helpers ────────────────────────────────────────────────────────────

    fn mock_registration() -> SourceRegistration {
        SourceRegistration {
            source_id:    "rss-replay".to_string(),
            source_type:  "REPLAY".to_string(),
            source_name:  "RSS Replay Simulator".to_string(),
            mission_id:   1,
            mission_name: "Chandrayaan-3".to_string(),
            mission_code: "cy3".to_string(),
            satellite_id:   101,
            satellite_name: "CY3-Orbiter".to_string(),
            norad_id:       57437,
            station_id:     10,
            station_name:   "ISRO ISTRAC".to_string(),
            station_code:   "istrac".to_string(),
            registered_at:  0,
            status: SourceStatus::Registered,
        }
    }

    fn print_report_static(stats: &SessionStats) {
        let rec   = stats.received.load(Ordering::SeqCst);
        let publ  = stats.published.load(Ordering::SeqCst);
        let drop  = stats.dropped.load(Ordering::SeqCst);

        let latencies = stats.latencies_ms.lock().unwrap().clone();
        let delays    = stats.delays_ms.lock().unwrap().clone();
        let n = latencies.len() as f64;

        let (avg_lat, min_lat, max_lat, jitter) = if n > 0.0 {
            let sum  = latencies.iter().sum::<f64>();
            let avg  = sum / n;
            let min  = latencies.iter().copied().fold(f64::INFINITY,     f64::min);
            let max  = latencies.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let var  = latencies.iter().map(|&x| (x - avg).powi(2)).sum::<f64>() / n;
            (avg, min, max, var.sqrt())
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };

        let avg_delay = if delays.is_empty() { 0.0 }
                        else { delays.iter().sum::<f64>() / delays.len() as f64 };

        println!("\n==================================================");
        println!("   REPLAY VERIFICATION REPORT (SPRINT 3)");
        println!("==================================================");
        println!(" Received  : {}", rec);
        println!(" Published : {}", publ);
        println!(" Dropped   : {}", drop);
        println!("--------------------------------------------------");
        println!(" Latency (Origin → Gateway Publish):");
        println!("   Avg     : {:.3} ms", avg_lat);
        println!("   Min     : {:.3} ms", min_lat);
        println!("   Max     : {:.3} ms", max_lat);
        println!("   Jitter  : {:.3} ms", jitter);
        println!("--------------------------------------------------");
        println!(" Avg Queue Delay : {:.3} ms", avg_delay);
        println!("==================================================\n");
    }
}

// ── IngestPort implementation ───────────────────────────────────────────────

#[tonic::async_trait]
impl IngestPort for IngestionOrchestrator {
    async fn on_packet_received(
        &self,
        mut envelope: TelemetryEnvelope,
    ) -> Result<(), GatewayError> {
        self.stats.received.fetch_add(1, Ordering::SeqCst);
        // Reset silence timer
        *self.last_packet_time.lock().unwrap() = Some(Instant::now());
        self.report_printed.store(false, Ordering::SeqCst);

        // ── 1. Normalize ────────────────────────────────────────────────
        Normalizer::normalize(&mut envelope);

        // ── 2. Validate ─────────────────────────────────────────────────
        if let Err(e) = Validator::validate(&envelope, true, true) {
            self.stats.dropped.fetch_add(1, Ordering::SeqCst);
            tracing::warn!("Packet dropped (validation): {:?}", e);
            return Err(e);
        }

        // ── 3. Enrich ───────────────────────────────────────────────────
        let reg = Self::mock_registration();
        let seq = self.seq_counter.fetch_add(1, Ordering::SeqCst);
        Enricher::enrich(&mut envelope, &reg, seq);

        // ── 4. Route ────────────────────────────────────────────────────
        let routing_key = Router::build_routing_key(&envelope);

        // ── 5. Publish Timestamp ────────────────────────────────────────
        Enricher::set_publish_timestamp(&mut envelope);

        // ── 6. Publish to RabbitMQ (or console if not connected) ────────
        let apid        = envelope.apid;
        let payload_len = envelope.raw_packet.as_ref().map(|p| p.data.len()).unwrap_or(0);
        let seq_num     = envelope.sequence_number;
        let env_id      = envelope.envelope_id.clone();

        if self.publish_port.is_connected() {
            match self.publish_port.publish(envelope.clone(), &routing_key).await {
                Ok(_) => {
                    tracing::info!(
                        "[RabbitMQ ✓] key={} | EnvID={} | Seq={} | APID={} | {} bytes",
                        routing_key, env_id, seq_num, apid, payload_len
                    );
                    self.stats.published.fetch_add(1, Ordering::SeqCst);
                }
                Err(e) => {
                    tracing::error!("[RabbitMQ ✗] Publish failed: {:?}", e);
                    self.stats.dropped.fetch_add(1, Ordering::SeqCst);
                    return Err(e);
                }
            }
        } else {
            // Fallback: console sink (broker not yet available)
            tracing::info!(
                "[Console Sink] key={} | EnvID={} | Seq={} | APID={} | {} bytes",
                routing_key, env_id, seq_num, apid, payload_len
            );
            self.stats.published.fetch_add(1, Ordering::SeqCst);
        }

        // ── 7. Latency / delay stats ────────────────────────────────────
        if let (Some(orig), Some(pub_ts)) =
            (&envelope.original_timestamp, &envelope.publish_timestamp)
        {
            let ms = pub_ts.nanos_since_epoch.saturating_sub(orig.nanos_since_epoch) as f64
                     / 1_000_000.0;
            self.stats.latencies_ms.lock().unwrap().push(ms);
        }
        if let (Some(rec_ts), Some(pub_ts)) =
            (&envelope.receive_timestamp, &envelope.publish_timestamp)
        {
            let ms = pub_ts.nanos_since_epoch.saturating_sub(rec_ts.nanos_since_epoch) as f64
                     / 1_000_000.0;
            self.stats.delays_ms.lock().unwrap().push(ms);
        }

        Ok(())
    }

    async fn on_source_connected(&self, source_id: &str) -> Result<(), GatewayError> {
        tracing::info!("Source connected: {}", source_id);
        self.stats.received.store(0, Ordering::SeqCst);
        self.stats.published.store(0, Ordering::SeqCst);
        self.stats.dropped.store(0, Ordering::SeqCst);
        self.stats.latencies_ms.lock().unwrap().clear();
        self.stats.delays_ms.lock().unwrap().clear();
        *self.last_packet_time.lock().unwrap() = None;
        self.report_printed.store(false, Ordering::SeqCst);
        Ok(())
    }

    async fn on_source_disconnected(&self, source_id: &str) -> Result<(), GatewayError> {
        tracing::info!("Source disconnected: {}", source_id);
        Self::print_report_static(&self.stats);
        Ok(())
    }

    async fn on_session_eof(&self, session_id: &str) -> Result<(), GatewayError> {
        tracing::info!("Session EOF: {}", session_id);
        Self::print_report_static(&self.stats);
        Ok(())
    }
}

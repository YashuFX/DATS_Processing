use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicI64, Ordering};
use crate::ports::MetricsPort;

/// MetricsPort adapter that publishes to the global metrics registry and stores states thread-safely.
pub struct PrometheusMetricsExporter {
    packets_published: AtomicU64,
    timing_jitter_ns: AtomicI64,
    playback_state: Mutex<String>,
    playback_speed: AtomicU64,    // Bit-representation of f64
    playback_progress: AtomicU64, // Bit-representation of f64
}

impl PrometheusMetricsExporter {
    /// Creates a new PrometheusMetricsExporter.
    pub fn new() -> Self {
        Self {
            packets_published: AtomicU64::new(0),
            timing_jitter_ns: AtomicI64::new(0),
            playback_state: Mutex::new("IDLE".to_string()),
            playback_speed: AtomicU64::new(1.0f64.to_bits()),
            playback_progress: AtomicU64::new(0.0f64.to_bits()),
        }
    }

    /// Retrieves packets published count.
    pub fn get_packets_published(&self) -> u64 {
        self.packets_published.load(Ordering::Relaxed)
    }

    /// Retrieves current logical drift in nanoseconds.
    pub fn get_timing_jitter_ns(&self) -> i64 {
        self.timing_jitter_ns.load(Ordering::Relaxed)
    }

    /// Retrieves current state string.
    pub fn get_playback_state(&self) -> String {
        self.playback_state.lock().unwrap().clone()
    }

    /// Retrieves current playback speed.
    pub fn get_playback_speed(&self) -> f64 {
        f64::from_bits(self.playback_speed.load(Ordering::Relaxed))
    }

    /// Retrieves current progress (0.0 to 1.0).
    pub fn get_playback_progress(&self) -> f64 {
        f64::from_bits(self.playback_progress.load(Ordering::Relaxed))
    }
}

impl MetricsPort for PrometheusMetricsExporter {
    fn record_packets_published(&self, count: u64) {
        self.packets_published.fetch_add(count, Ordering::Relaxed);
        metrics::counter!("rss_packets_published_total").increment(count);
    }

    fn record_timing_jitter(&self, jitter_ns: i64) {
        self.timing_jitter_ns.store(jitter_ns, Ordering::Relaxed);
        metrics::gauge!("rss_timing_jitter_nanoseconds").set(jitter_ns as f64);
    }

    fn record_command(&self, command: &str, success: bool) {
        let success_str = if success { "true" } else { "false" };
        metrics::counter!("rss_commands_total", "command" => command.to_string(), "success" => success_str).increment(1);
    }

    fn set_playback_state(&self, state: &str) {
        {
            let mut guard = self.playback_state.lock().unwrap();
            *guard = state.to_string();
        }
        // Expose state as label or gauge
        tracing::info!("Metrics: Playback state set to {}", state);
    }

    fn set_playback_speed(&self, speed: f64) {
        self.playback_speed.store(speed.to_bits(), Ordering::Relaxed);
        metrics::gauge!("rss_playback_speed_ratio").set(speed);
    }

    fn set_progress(&self, progress: f64) {
        self.playback_progress.store(progress.to_bits(), Ordering::Relaxed);
        metrics::gauge!("rss_playback_progress_percentage").set(progress * 100.0);
    }
}

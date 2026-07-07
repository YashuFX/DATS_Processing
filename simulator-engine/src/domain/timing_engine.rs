use std::time::Duration;
use tokio::time::Instant;

/// The TimingEngine manages the logical playback clock, speed scaling,
/// and drift correction to preserve original recording packet arrival patterns.
#[derive(Debug)]
pub struct TimingEngine {
    speed: f64,
    drift_correction_enabled: bool,

    // Session markers (established at start or reset/seek)
    session_start: Option<Instant>,
    first_packet_timestamp_ns: Option<u64>,
    last_packet_timestamp_ns: Option<u64>,

    // Pause tracking
    pause_start: Option<Instant>,
    total_paused_duration: Duration,

    // Observability stats
    total_drift_corrected_ns: i64,
    drift_correction_count: u64,
}

impl TimingEngine {
    /// Creates a new TimingEngine.
    pub fn new(drift_correction_enabled: bool) -> Self {
        Self {
            speed: 1.0,
            drift_correction_enabled,
            session_start: None,
            first_packet_timestamp_ns: None,
            last_packet_timestamp_ns: None,
            pause_start: None,
            total_paused_duration: Duration::ZERO,
            total_drift_corrected_ns: 0,
            drift_correction_count: 0,
        }
    }

    /// Initializes the timing engine with the first packet's timestamp and desired playback speed.
    pub fn initialize(&mut self, first_packet_ts_ns: u64, speed: f64) {
        self.speed = if speed <= 0.0 { 1.0 } else { speed };
        self.session_start = Some(Instant::now());
        self.first_packet_timestamp_ns = Some(first_packet_ts_ns);
        self.last_packet_timestamp_ns = Some(first_packet_ts_ns);
        self.pause_start = None;
        self.total_paused_duration = Duration::ZERO;
        self.total_drift_corrected_ns = 0;
        self.drift_correction_count = 0;
    }

    /// Freezes the timing engine clock on pause.
    pub fn pause(&mut self) {
        if self.pause_start.is_none() && self.session_start.is_some() {
            self.pause_start = Some(Instant::now());
        }
    }

    /// Resumes the timing engine clock.
    pub fn resume(&mut self) {
        if let Some(p_start) = self.pause_start.take() {
            let paused_elapsed = p_start.elapsed();
            self.total_paused_duration += paused_elapsed;
        }
    }

    /// Updates the playback speed dynamically.
    /// Resets drift accumulators to establish a new playback segment at the new speed.
    pub fn set_speed(&mut self, new_speed: f64, current_ts_ns: u64) {
        self.speed = if new_speed <= 0.0 { 1.0 } else { new_speed };
        // If we are currently running, reset segment to avoid drift correction skew
        if self.session_start.is_some() {
            self.session_start = Some(Instant::now());
            self.first_packet_timestamp_ns = Some(current_ts_ns);
            self.last_packet_timestamp_ns = Some(current_ts_ns);
            self.total_paused_duration = Duration::ZERO;
            // Note: Keep cumulative stats for observability
        }
    }

    /// Resets the timing engine (e.g. after a Seek).
    pub fn reset(&mut self, target_ts_ns: u64) {
        if self.session_start.is_some() {
            self.session_start = Some(Instant::now());
            self.first_packet_timestamp_ns = Some(target_ts_ns);
            self.last_packet_timestamp_ns = Some(target_ts_ns);
            self.total_paused_duration = Duration::ZERO;
        }
    }

    /// Checks if paused.
    pub fn is_paused(&self) -> bool {
        self.pause_start.is_some()
    }

    /// Computes the speed-adjusted sleep duration for the next packet.
    /// Incorporates drift correction to prevent processing overhead from lagging the stream.
    pub fn compute_delay(&mut self, packet_ts_ns: u64) -> Duration {
        let last_ts = match self.last_packet_timestamp_ns {
            Some(ts) => ts,
            None => {
                self.last_packet_timestamp_ns = Some(packet_ts_ns);
                return Duration::ZERO;
            }
        };

        // If the packet timestamp went backwards (non-monotonic), fallback to last ts + 1ms equivalent delta
        let original_delta_ns = if packet_ts_ns >= last_ts {
            packet_ts_ns - last_ts
        } else {
            1_000_000 // 1 millisecond fallback
        };

        // Update last packet timestamp
        self.last_packet_timestamp_ns = Some(packet_ts_ns);

        // Base delay scaled by speed
        let base_delay_ns = (original_delta_ns as f64 / self.speed) as u64;

        if !self.drift_correction_enabled {
            return Duration::from_nanos(base_delay_ns);
        }

        // Drift calculation:
        // Expected elapsed time in nanoseconds since start of this segment
        let first_ts = self.first_packet_timestamp_ns.unwrap_or(packet_ts_ns);
        let expected_elapsed_ns = if packet_ts_ns >= first_ts {
            ((packet_ts_ns - first_ts) as f64 / self.speed) as u64
        } else {
            0
        };

        // Actual real time elapsed since segment start, minus pause intervals
        let start_time = self.session_start.unwrap_or_else(Instant::now);
        let current_real_elapsed = start_time.elapsed();
        let active_real_elapsed = current_real_elapsed.saturating_sub(self.total_paused_duration);
        let active_real_elapsed_ns = active_real_elapsed.as_nanos() as u64;

        // Drift = real_elapsed - expected_elapsed
        // Positive drift means we are running slow (real elapsed > expected elapsed)
        // Negative drift means we are running fast (real elapsed < expected elapsed)
        let drift_ns = active_real_elapsed_ns as i64 - expected_elapsed_ns as i64;

        // Adjust the delay: if slow, reduce delay; if fast, increase delay
        let adjusted_delay_ns = if drift_ns > 0 {
            base_delay_ns.saturating_sub(drift_ns as u64)
        } else {
            base_delay_ns.saturating_add((-drift_ns) as u64)
        };

        // Accumulate drift correction stats
        self.total_drift_corrected_ns += drift_ns;
        self.drift_correction_count += 1;

        Duration::from_nanos(adjusted_delay_ns)
    }

    /// Gets cumulative timing statistics.
    pub fn statistics(&self) -> (i64, u64) {
        (self.total_drift_corrected_ns, self.drift_correction_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_timing_engine_initialization() {
        let mut engine = TimingEngine::new(true);
        engine.initialize(1000, 2.0);
        assert_eq!(engine.speed, 2.0);
        assert_eq!(engine.first_packet_timestamp_ns, Some(1000));
        assert_eq!(engine.last_packet_timestamp_ns, Some(1000));
    }

    #[test]
    fn test_compute_delay_no_drift() {
        let mut engine = TimingEngine::new(false);
        engine.initialize(1000, 1.0);

        // First delay calculation is zero as it establishes last timestamp
        let delay1 = engine.compute_delay(1000);
        assert_eq!(delay1, Duration::ZERO);

        // Next packet is 10ms later
        let delay2 = engine.compute_delay(10_001_000); // +10ms in ns
        assert_eq!(delay2, Duration::from_millis(10));
    }

    #[test]
    fn test_compute_delay_with_speed() {
        let mut engine = TimingEngine::new(false);
        engine.initialize(1000, 2.0); // 2x speed

        let _ = engine.compute_delay(1000);
        let delay = engine.compute_delay(10_001_000); // 10ms delta at 2x speed -> should be 5ms delay
        assert_eq!(delay, Duration::from_millis(5));
    }

    #[test]
    fn test_timing_engine_pause_resume() {
        let mut engine = TimingEngine::new(true);
        engine.initialize(1000, 1.0);

        engine.pause();
        assert!(engine.is_paused());

        // Wait a short duration
        std::thread::sleep(Duration::from_millis(5));
        engine.resume();
        assert!(!engine.is_paused());
        assert!(engine.total_paused_duration >= Duration::from_millis(5));
    }

    #[test]
    fn test_timing_engine_reset() {
        let mut engine = TimingEngine::new(true);
        engine.initialize(1000, 1.0);
        engine.reset(5000);
        assert_eq!(engine.first_packet_timestamp_ns, Some(5000));
        assert_eq!(engine.last_packet_timestamp_ns, Some(5000));
    }
}

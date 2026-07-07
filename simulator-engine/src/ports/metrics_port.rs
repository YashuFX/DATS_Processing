pub trait MetricsPort: Send + Sync {
    /// Records the total number of packets successfully published.
    fn record_packets_published(&self, count: u64);

    /// Records the time drift/jitter between logical play clock and wall clock in nanoseconds.
    fn record_timing_jitter(&self, jitter_ns: i64);

    /// Records invocation success/failure rates of operational commands.
    fn record_command(&self, command: &str, success: bool);

    /// Exposes the current scheduler status string to prometheus.
    fn set_playback_state(&self, state: &str);

    /// Records the current speed multiplier.
    fn set_playback_speed(&self, speed: f64);

    /// Records percentage progress through the current file replay session.
    fn set_progress(&self, progress: f64);
}

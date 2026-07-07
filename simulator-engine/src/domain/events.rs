use crate::domain::state_machine::PlaybackState;

/// Internal domain events emitted by the Replay Simulator.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplayEvent {
    PlaybackStarted {
        speed: f64,
        loop_enabled: bool,
    },
    PlaybackPaused {
        paused_at_packet: u64,
        paused_at_timestamp_ns: u64,
    },
    PlaybackResumed {
        resumed_at_packet: u64,
    },
    PlaybackFinished,
    PlaybackError {
        error_message: String,
        recoverable: bool,
    },
    StatusChanged {
        from: PlaybackState,
        to: PlaybackState,
    },
}

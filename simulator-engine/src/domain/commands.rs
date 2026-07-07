/// Represents all operational commands that can be issued to the replay engine.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplayCommand {
    /// Loads a telemetry recording file.
    LoadFile {
        file_path: String,
        file_type: String, // "binary" or "ccsds"
        target_stage: i32,
    },
    /// Starts or restarts playback.
    Start {
        speed: f64,
        loop_enabled: bool,
    },
    /// Pauses playback.
    Pause,
    /// Resumes playback from pause.
    Resume,
    /// Seeks to a specific timestamp in the telemetry pass (in nanoseconds).
    Seek {
        target_timestamp_ns: u64,
    },
    /// Stops playback and resets progress.
    Stop,
    /// Unloads the file and returns to IDLE.
    UnloadFile,
    /// Changes playback speed on the fly.
    SetSpeed {
        speed: f64,
    },
    /// Toggles loop mode.
    SetLoop {
        enabled: bool,
    },
}

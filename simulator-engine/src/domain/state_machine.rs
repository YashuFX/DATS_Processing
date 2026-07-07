use crate::domain::commands::ReplayCommand;
use crate::domain::errors::ReplayError;
use serde::{Serialize, Deserialize};

/// States representing the current execution phase of the replay loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlaybackState {
    IDLE,
    READY,
    RUNNING,
    PAUSED,
    STOPPED,
    COMPLETED,
    ERROR,
}

impl PlaybackState {
    /// Returns the string representation of the playback state.
    pub fn as_str(&self) -> &'static str {
        match self {
            PlaybackState::IDLE => "IDLE",
            PlaybackState::READY => "READY",
            PlaybackState::RUNNING => "RUNNING",
            PlaybackState::PAUSED => "PAUSED",
            PlaybackState::STOPPED => "STOPPED",
            PlaybackState::COMPLETED => "COMPLETED",
            PlaybackState::ERROR => "ERROR",
        }
    }
}

/// A state machine coordinating valid transitions and states.
#[derive(Debug, Clone)]
pub struct StateMachine {
    current: PlaybackState,
}

impl StateMachine {
    /// Creates a new state machine initialized to IDLE.
    pub fn new() -> Self {
        Self {
            current: PlaybackState::IDLE,
        }
    }

    /// Gets the current state.
    pub fn current_state(&self) -> PlaybackState {
        self.current
    }

    /// Sets the state.
    pub fn set_state(&mut self, next: PlaybackState) {
        self.current = next;
    }

    /// Validates if a transition is legal for the given command.
    /// Returns the target state if valid, or an InvalidTransition error if not.
    pub fn validate_transition(&self, command: &ReplayCommand) -> Result<PlaybackState, ReplayError> {
        let current = self.current;
        match (current, command) {
            // IDLE state transitions
            (PlaybackState::IDLE, ReplayCommand::LoadFile { .. }) => Ok(PlaybackState::READY),
            
            // READY state transitions
            (PlaybackState::READY, ReplayCommand::Start { .. }) => Ok(PlaybackState::RUNNING),
            (PlaybackState::READY, ReplayCommand::UnloadFile) => Ok(PlaybackState::IDLE),
            (PlaybackState::READY, ReplayCommand::Seek { .. }) => Ok(PlaybackState::READY),
            (PlaybackState::READY, ReplayCommand::SetSpeed { .. }) => Ok(PlaybackState::READY),
            (PlaybackState::READY, ReplayCommand::SetLoop { .. }) => Ok(PlaybackState::READY),

            // RUNNING state transitions
            (PlaybackState::RUNNING, ReplayCommand::Pause) => Ok(PlaybackState::PAUSED),
            (PlaybackState::RUNNING, ReplayCommand::Stop) => Ok(PlaybackState::STOPPED),
            (PlaybackState::RUNNING, ReplayCommand::SetSpeed { .. }) => Ok(PlaybackState::RUNNING),
            (PlaybackState::RUNNING, ReplayCommand::SetLoop { .. }) => Ok(PlaybackState::RUNNING),

            // PAUSED state transitions
            (PlaybackState::PAUSED, ReplayCommand::Resume) => Ok(PlaybackState::RUNNING),
            (PlaybackState::PAUSED, ReplayCommand::Stop) => Ok(PlaybackState::STOPPED),
            (PlaybackState::PAUSED, ReplayCommand::Seek { .. }) => Ok(PlaybackState::PAUSED),
            (PlaybackState::PAUSED, ReplayCommand::SetSpeed { .. }) => Ok(PlaybackState::PAUSED),
            (PlaybackState::PAUSED, ReplayCommand::SetLoop { .. }) => Ok(PlaybackState::PAUSED),

            // STOPPED state transitions
            (PlaybackState::STOPPED, ReplayCommand::Start { .. }) => Ok(PlaybackState::RUNNING),
            (PlaybackState::STOPPED, ReplayCommand::LoadFile { .. }) => Ok(PlaybackState::READY),
            (PlaybackState::STOPPED, ReplayCommand::UnloadFile) => Ok(PlaybackState::IDLE),
            (PlaybackState::STOPPED, ReplayCommand::Seek { .. }) => Ok(PlaybackState::STOPPED),
            (PlaybackState::STOPPED, ReplayCommand::SetSpeed { .. }) => Ok(PlaybackState::STOPPED),

            // COMPLETED state transitions
            (PlaybackState::COMPLETED, ReplayCommand::Start { .. }) => Ok(PlaybackState::RUNNING),
            (PlaybackState::COMPLETED, ReplayCommand::LoadFile { .. }) => Ok(PlaybackState::READY),
            (PlaybackState::COMPLETED, ReplayCommand::UnloadFile) => Ok(PlaybackState::IDLE),

            // ERROR state transitions
            (PlaybackState::ERROR, ReplayCommand::LoadFile { .. }) => Ok(PlaybackState::READY),
            (PlaybackState::ERROR, ReplayCommand::UnloadFile) => Ok(PlaybackState::IDLE),

            // Catch-all invalid transitions
            (state, cmd) => Err(ReplayError::InvalidTransition {
                current: state.as_str().to_string(),
                event: format!("{:?}", cmd),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let fsm = StateMachine::new();
        assert_eq!(fsm.current_state(), PlaybackState::IDLE);
    }

    #[test]
    fn test_valid_transitions() {
        let mut fsm = StateMachine::new();

        // IDLE -> LoadFile -> READY
        let load_cmd = ReplayCommand::LoadFile {
            file_path: "test.bin".to_string(),
            file_type: "binary".to_string(),
            target_stage: 0,
        };
        let next = fsm.validate_transition(&load_cmd).unwrap();
        assert_eq!(next, PlaybackState::READY);
        fsm.set_state(next);

        // READY -> Start -> RUNNING
        let start_cmd = ReplayCommand::Start { speed: 1.0, loop_enabled: false };
        let next = fsm.validate_transition(&start_cmd).unwrap();
        assert_eq!(next, PlaybackState::RUNNING);
        fsm.set_state(next);

        // RUNNING -> Pause -> PAUSED
        let next = fsm.validate_transition(&ReplayCommand::Pause).unwrap();
        assert_eq!(next, PlaybackState::PAUSED);
        fsm.set_state(next);

        // PAUSED -> Resume -> RUNNING
        let next = fsm.validate_transition(&ReplayCommand::Resume).unwrap();
        assert_eq!(next, PlaybackState::RUNNING);
        fsm.set_state(next);

        // RUNNING -> Stop -> STOPPED
        let next = fsm.validate_transition(&ReplayCommand::Stop).unwrap();
        assert_eq!(next, PlaybackState::STOPPED);
        fsm.set_state(next);

        // STOPPED -> UnloadFile -> IDLE
        let next = fsm.validate_transition(&ReplayCommand::UnloadFile).unwrap();
        assert_eq!(next, PlaybackState::IDLE);
    }

    #[test]
    fn test_invalid_transitions() {
        let fsm = StateMachine::new();

        // Cannot pause while IDLE
        let res = fsm.validate_transition(&ReplayCommand::Pause);
        assert!(res.is_err());

        // Cannot seek while RUNNING
        let mut running_fsm = StateMachine::new();
        running_fsm.set_state(PlaybackState::RUNNING);
        let seek_cmd = ReplayCommand::Seek { target_timestamp_ns: 1000 };
        let res = running_fsm.validate_transition(&seek_cmd);
        assert!(res.is_err());
    }
}

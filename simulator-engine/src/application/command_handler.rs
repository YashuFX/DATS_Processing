use std::sync::Arc;
use tokio::sync::Mutex;
use crate::application::orchestrator::ReplayOrchestrator;
use crate::domain::commands::ReplayCommand;
use crate::domain::errors::ReplayError;
use crate::domain::models::SourceMetadata;
use crate::domain::state_machine::PlaybackState;

/// CommandHandler is the primary application service.
/// It receives commands from driving adapters and runs them against the orchestrator.
pub struct CommandHandler {
    orchestrator: Arc<Mutex<ReplayOrchestrator>>,
}

impl CommandHandler {
    /// Creates a new CommandHandler.
    pub fn new(orchestrator: Arc<Mutex<ReplayOrchestrator>>) -> Self {
        Self { orchestrator }
    }

    /// Handles a replay command.
    pub async fn handle(&self, command: ReplayCommand) -> Result<Option<SourceMetadata>, ReplayError> {
        ReplayOrchestrator::execute_command(&self.orchestrator, command).await
    }

    /// Queries the orchestrator for current playback status.
    pub async fn get_status(&self) -> (PlaybackState, f64, f64, u64, u64) {
        let orch = self.orchestrator.lock().await;
        orch.get_status()
    }

    /// Queries the orchestrator for recently published packets.
    pub async fn get_recent_packets(&self) -> Vec<crate::application::orchestrator::RecentPacketLog> {
        let orch = self.orchestrator.lock().await;
        orch.recent_packets.clone()
    }
}

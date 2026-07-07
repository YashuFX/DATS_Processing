use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, Mutex};
use crate::domain::state_machine::{StateMachine, PlaybackState};
use crate::domain::commands::ReplayCommand;
use crate::domain::errors::ReplayError;
use crate::domain::models::SourceMetadata;
use crate::ports::{SourcePort, PublishPort, EventPort, MetricsPort};
use crate::domain::replay_scheduler::{ReplayScheduler, SchedulerCommand, SchedulerNotification};
use crate::api::events::v1::{PlatformEvent, EventSeverity};
use crate::api::common::v1::{
    SourceIdentifier, SourceType, MustTimestamp, TimestampSource,
    MissionIdentifier, SatelliteIdentifier, GroundStationIdentifier,
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct RecentPacketLog {
    pub sequence_number: u64,
    pub timestamp_ns: u64,
    pub apid: Option<u16>,
}

pub struct ReplayOrchestrator {
    state_machine: StateMachine,
    source_port: Arc<Mutex<dyn SourcePort>>,
    publish_port: Arc<dyn PublishPort>,
    metrics_port: Arc<dyn MetricsPort>,
    event_port: Arc<dyn EventPort>,

    // Playback state variables
    speed: f64,
    loop_enabled: bool,
    target_stage: i32,
    file_type: String,
    file_path: String,
    source_metadata: Option<SourceMetadata>,

    // Metadata configured at startup or defaults
    mission: Option<MissionIdentifier>,
    satellite: Option<SatelliteIdentifier>,
    station: Option<GroundStationIdentifier>,

    // Scheduler task handle
    scheduler_tx: Option<mpsc::Sender<SchedulerCommand>>,

    // Progress stats
    pub packets_published: u64,
    pub current_timestamp_ns: u64,
    pub recent_packets: Vec<RecentPacketLog>,
}

impl ReplayOrchestrator {
    /// Creates a new ReplayOrchestrator.
    pub fn new(
        source_port: Arc<Mutex<dyn SourcePort>>,
        publish_port: Arc<dyn PublishPort>,
        metrics_port: Arc<dyn MetricsPort>,
        event_port: Arc<dyn EventPort>,
        mission: Option<MissionIdentifier>,
        satellite: Option<SatelliteIdentifier>,
        station: Option<GroundStationIdentifier>,
    ) -> Self {
        Self {
            state_machine: StateMachine::new(),
            source_port,
            publish_port,
            metrics_port,
            event_port,
            speed: 1.0,
            loop_enabled: false,
            target_stage: 0,
            file_type: String::new(),
            file_path: String::new(),
            source_metadata: None,
            mission,
            satellite,
            station,
            scheduler_tx: None,
            packets_published: 0,
            current_timestamp_ns: 0,
            recent_packets: Vec::new(),
        }
    }

    /// Gets the current state machine state.
    pub fn current_state(&self) -> PlaybackState {
        self.state_machine.current_state()
    }

    /// Retrieves full playback status for external display.
    pub fn get_status(&self) -> (PlaybackState, f64, f64, u64, u64) {
        let state = self.state_machine.current_state();
        let progress = if let Some(ref meta) = self.source_metadata {
            if meta.duration_ns > 0 {
                let elapsed = self.current_timestamp_ns.saturating_sub(meta.start_timestamp_ns);
                (elapsed as f64 / meta.duration_ns as f64).clamp(0.0, 1.0)
            } else {
                0.0
            }
        } else {
            0.0
        };

        (
            state,
            self.speed,
            progress,
            self.packets_published,
            self.current_timestamp_ns,
        )
    }

    /// Executes an operational command, updating the state machine and controlling the scheduler.
    /// This method is called from the command handler and is run within a lock.
    pub async fn execute_command(
        self_arc: &Arc<Mutex<Self>>,
        command: ReplayCommand,
    ) -> Result<Option<SourceMetadata>, ReplayError> {
        // 1. Lock and validate transition first
        let mut orch = self_arc.lock().await;
        let next_state = orch.state_machine.validate_transition(&command)?;
        let prev_state = orch.state_machine.current_state();

        tracing::info!(
            "Command {:?} triggering transition: {} -> {}",
            command,
            prev_state.as_str(),
            next_state.as_str()
        );

        let mut result_meta = None;

        // 2. Perform command action
        match command {
            ReplayCommand::LoadFile { file_path, file_type, target_stage } => {
                let metadata = {
                    let mut src = orch.source_port.lock().await;
                    src.open(&file_path, &file_type)?
                };

                orch.file_path = file_path;
                orch.file_type = file_type;
                orch.target_stage = target_stage;
                orch.source_metadata = Some(metadata.clone());
                orch.current_timestamp_ns = metadata.start_timestamp_ns;
                orch.packets_published = 0;
                orch.recent_packets.clear();
                result_meta = Some(metadata);
            }
            ReplayCommand::Start { speed, loop_enabled } => {
                orch.speed = speed;
                orch.loop_enabled = loop_enabled;

                // Set up channel for controlling scheduler task
                let (cmd_tx, cmd_rx) = mpsc::channel(128);
                let (notify_tx, mut notify_rx) = mpsc::channel(128);

                let target_stage = match orch.target_stage {
                    1 => crate::api::telemetry::v1::ProcessingStage::Raw,
                    2 => crate::api::telemetry::v1::ProcessingStage::CcsdsDecoded,
                    3 => crate::api::telemetry::v1::ProcessingStage::Engineering,
                    4 => crate::api::telemetry::v1::ProcessingStage::Validated,
                    5 => crate::api::telemetry::v1::ProcessingStage::Archived,
                    _ => crate::api::telemetry::v1::ProcessingStage::Raw,
                };

                let builder_config = crate::domain::envelope_builder::EnvelopeBuilderConfig {
                    source_id: "rss-replay".to_string(),
                    source_name: "Replay Simulator Service".to_string(),
                    target_stage,
                    mission: orch.mission.clone(),
                    satellite: orch.satellite.clone(),
                    station: orch.station.clone(),
                };
                let envelope_builder = crate::domain::envelope_builder::EnvelopeBuilder::new(builder_config);

                let scheduler = ReplayScheduler::new(
                    Arc::clone(&orch.source_port),
                    Arc::clone(&orch.publish_port),
                    Arc::clone(&orch.metrics_port),
                    Arc::clone(&orch.event_port),
                    cmd_rx,
                    notify_tx,
                    orch.speed,
                    orch.loop_enabled,
                    envelope_builder,
                );

                // Spawn scheduler task in background
                tokio::spawn(scheduler.run());
                orch.scheduler_tx = Some(cmd_tx);

                // Spawn background notification listener
                let self_clone = Arc::clone(self_arc);
                tokio::spawn(async move {
                    while let Some(notification) = notify_rx.recv().await {
                        let mut o = self_clone.lock().await;
                        match notification {
                            SchedulerNotification::Eof => {
                                tracing::info!("Scheduler reported EOF");
                                o.handle_scheduler_eof();
                            }
                            SchedulerNotification::FatalError(err) => {
                                tracing::error!("Scheduler reported fatal error: {}", err);
                                o.handle_scheduler_error(&err);
                            }
                            SchedulerNotification::PacketPublished { sequence_number, timestamp_ns, apid } => {
                                o.packets_published = sequence_number;
                                o.current_timestamp_ns = timestamp_ns;
                                o.metrics_port.set_progress(o.get_status().2);
                                o.recent_packets.push(RecentPacketLog {
                                    sequence_number,
                                    timestamp_ns,
                                    apid,
                                });
                                if o.recent_packets.len() > 100 {
                                    o.recent_packets.remove(0);
                                }
                            }
                        }
                    }
                });

                orch.metrics_port.set_playback_speed(orch.speed);
            }
            ReplayCommand::Pause => {
                if let Some(ref tx) = orch.scheduler_tx {
                    let _ = tx.send(SchedulerCommand::Pause).await;
                }
            }
            ReplayCommand::Resume => {
                if let Some(ref tx) = orch.scheduler_tx {
                    let _ = tx.send(SchedulerCommand::Resume).await;
                }
            }
            ReplayCommand::Seek { target_timestamp_ns } => {
                if let Some(ref tx) = orch.scheduler_tx {
                    let _ = tx.send(SchedulerCommand::Seek { timestamp_ns: target_timestamp_ns }).await;
                } else {
                    let mut src = orch.source_port.lock().await;
                    src.seek(target_timestamp_ns)?;
                }
                orch.current_timestamp_ns = target_timestamp_ns;
            }
            ReplayCommand::Stop => {
                if let Some(ref tx) = orch.scheduler_tx.take() {
                    let _ = tx.send(SchedulerCommand::Stop).await;
                }
                orch.packets_published = 0;
                if let Some(ref meta) = orch.source_metadata {
                    let start_ts = meta.start_timestamp_ns;
                    orch.current_timestamp_ns = start_ts;
                    let mut src = orch.source_port.lock().await;
                    let _ = src.seek(start_ts);
                }
            }
            ReplayCommand::UnloadFile => {
                if let Some(ref tx) = orch.scheduler_tx.take() {
                    let _ = tx.send(SchedulerCommand::Stop).await;
                }
                {
                    let mut src = orch.source_port.lock().await;
                    src.close()?;
                }
                orch.source_metadata = None;
                orch.file_path = String::new();
                orch.file_type = String::new();
                orch.packets_published = 0;
                orch.current_timestamp_ns = 0;
                orch.recent_packets.clear();
            }
            ReplayCommand::SetSpeed { speed } => {
                orch.speed = speed;
                if let Some(ref tx) = orch.scheduler_tx {
                    let _ = tx.send(SchedulerCommand::SetSpeed { speed }).await;
                }
                orch.metrics_port.set_playback_speed(orch.speed);
            }
            ReplayCommand::SetLoop { enabled } => {
                orch.loop_enabled = enabled;
                if let Some(ref tx) = orch.scheduler_tx {
                    let _ = tx.send(SchedulerCommand::SetLoop { enabled }).await;
                }
            }
        }

        // 3. Complete FSM state change and report metrics/events
        orch.state_machine.set_state(next_state);
        orch.metrics_port.set_playback_state(next_state.as_str());

        // Emit platform event for state changes
        let _ = orch.event_port.emit(PlatformEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            event_type: "playback_state_changed".to_string(),
            severity: EventSeverity::Info as i32,
            source: Some(SourceIdentifier {
                source_id: "rss-replay".to_string(),
                source_type: SourceType::Replay as i32,
                source_name: "Replay Simulator Service".to_string(),
            }),
            timestamp: Some(MustTimestamp {
                nanos_since_epoch: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64,
                source: TimestampSource::Replay as i32,
            }),
            message: format!(
                "Replay state transitioned from {} to {}",
                prev_state.as_str(),
                next_state.as_str()
            ),
            metadata: std::collections::HashMap::new(),
        });

        Ok(result_meta)
    }

    /// Internal transition upon normal EOF.
    fn handle_scheduler_eof(&mut self) {
        self.state_machine.set_state(PlaybackState::COMPLETED);
        self.metrics_port.set_playback_state(PlaybackState::COMPLETED.as_str());
        self.scheduler_tx = None;

        let _ = self.event_port.emit(PlatformEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            event_type: "playback_finished".to_string(),
            severity: EventSeverity::Info as i32,
            source: Some(SourceIdentifier {
                source_id: "rss-replay".to_string(),
                source_type: SourceType::Replay as i32,
                source_name: "Replay Simulator Service".to_string(),
            }),
            timestamp: Some(MustTimestamp {
                nanos_since_epoch: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64,
                source: TimestampSource::Replay as i32,
            }),
            message: "Replay playback finished normally (EOF)".to_string(),
            metadata: std::collections::HashMap::new(),
        });
    }

    /// Internal transition upon fatal error from scheduler.
    fn handle_scheduler_error(&mut self, err_msg: &str) {
        self.state_machine.set_state(PlaybackState::ERROR);
        self.metrics_port.set_playback_state(PlaybackState::ERROR.as_str());
        self.scheduler_tx = None;

        let _ = self.event_port.emit(PlatformEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            event_type: "playback_error".to_string(),
            severity: EventSeverity::Error as i32,
            source: Some(SourceIdentifier {
                source_id: "rss-replay".to_string(),
                source_type: SourceType::Replay as i32,
                source_name: "Replay Simulator Service".to_string(),
            }),
            timestamp: Some(MustTimestamp {
                nanos_since_epoch: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64,
                source: TimestampSource::Replay as i32,
            }),
            message: format!("Replay fatal error: {}", err_msg),
            metadata: std::collections::HashMap::new(),
        });
    }
}

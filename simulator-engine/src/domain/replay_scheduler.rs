use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{Instant, Duration};
use crate::ports::{SourcePort, PublishPort, EventPort, MetricsPort};
use crate::domain::timing_engine::TimingEngine;
use crate::domain::errors::ReplayError;
use crate::domain::replay_packet::ReplayPacket;
use crate::domain::envelope_builder::EnvelopeBuilder;
use crate::api::telemetry::v1::TelemetryEnvelope;

/// Control commands sent from Orchestrator to the running Scheduler task.
#[derive(Debug, Clone, PartialEq)]
pub enum SchedulerCommand {
    Pause,
    Resume,
    Seek { timestamp_ns: u64 },
    SetSpeed { speed: f64 },
    SetLoop { enabled: bool },
    Stop,
}

/// Notifications sent from the Scheduler task back to the Orchestrator.
#[derive(Debug, Clone)]
pub enum SchedulerNotification {
    Eof,
    FatalError(String),
    PacketPublished {
        sequence_number: u64,
        timestamp_ns: u64,
        apid: Option<u16>,
    },
}

/// ReplayScheduler encapsulates the execution state of the playback loop.
pub struct ReplayScheduler {
    source_port: Arc<Mutex<dyn SourcePort>>,
    publish_port: Arc<dyn PublishPort>,
    metrics_port: Arc<dyn MetricsPort>,
    event_port: Arc<dyn EventPort>,
    cmd_rx: mpsc::Receiver<SchedulerCommand>,
    notify_tx: mpsc::Sender<SchedulerNotification>,

    speed: f64,
    loop_enabled: bool,
    envelope_builder: EnvelopeBuilder,
}

impl ReplayScheduler {
    /// Creates a new ReplayScheduler instance.
    pub fn new(
        source_port: Arc<Mutex<dyn SourcePort>>,
        publish_port: Arc<dyn PublishPort>,
        metrics_port: Arc<dyn MetricsPort>,
        event_port: Arc<dyn EventPort>,
        cmd_rx: mpsc::Receiver<SchedulerCommand>,
        notify_tx: mpsc::Sender<SchedulerNotification>,
        speed: f64,
        loop_enabled: bool,
        envelope_builder: EnvelopeBuilder,
    ) -> Self {
        Self {
            source_port,
            publish_port,
            metrics_port,
            event_port,
            cmd_rx,
            notify_tx,
            speed,
            loop_enabled,
            envelope_builder,
        }
    }

    /// Spawns and executes the scheduler loop in the background.
    pub async fn run(mut self) {
        let mut timing_engine = TimingEngine::new(true);
        let mut is_running = true;
        let mut is_paused = false;
        let mut paused_remaining_delay: Option<Duration> = None;
        let mut current_packet: Option<ReplayPacket> = None;

        // Initialize timing engine with the first packet's timestamp
        let first_packet = {
            let mut src = self.source_port.lock().await;
            match src.read_next_packet() {
                Ok(Some(pkt)) => {
                    // Reset position back to start
                    if let Err(e) = src.seek(pkt.original_timestamp_ns) {
                        let _ = self.notify_tx.send(SchedulerNotification::FatalError(
                            format!("Failed to seek back to start: {}", e)
                        )).await;
                        return;
                    }
                    Some(pkt)
                }
                Ok(None) => None,
                Err(e) => {
                    let _ = self.notify_tx.send(SchedulerNotification::FatalError(
                        format!("Failed to read first packet: {}", e)
                    )).await;
                    return;
                }
            }
        };

        if let Some(ref pkt) = first_packet {
            timing_engine.initialize(pkt.original_timestamp_ns, self.speed);
        } else {
            let _ = self.notify_tx.send(SchedulerNotification::Eof).await;
            return;
        }

        while is_running {
            if is_paused {
                // When paused, we block exclusively waiting for a control command
                match self.cmd_rx.recv().await {
                    Some(SchedulerCommand::Resume) => {
                        is_paused = false;
                        timing_engine.resume();
                        tracing::info!("Playback resumed");
                    }
                    Some(SchedulerCommand::Stop) => {
                        is_running = false;
                        tracing::info!("Playback stopped during pause");
                    }
                    Some(SchedulerCommand::Seek { timestamp_ns }) => {
                        let mut src = self.source_port.lock().await;
                        if let Err(e) = src.seek(timestamp_ns) {
                            tracing::error!("Seek failed during pause: {}", e);
                        }
                        timing_engine.reset(timestamp_ns);
                        current_packet = None;
                        paused_remaining_delay = None;
                        tracing::info!("Seek to {} ns performed during pause", timestamp_ns);
                    }
                    Some(SchedulerCommand::SetSpeed { speed }) => {
                        self.speed = speed;
                        if let Some(ref pkt) = current_packet {
                            timing_engine.set_speed(speed, pkt.original_timestamp_ns);
                        }
                        tracing::info!("Speed changed to {}x during pause", speed);
                    }
                    Some(SchedulerCommand::SetLoop { enabled }) => {
                        self.loop_enabled = enabled;
                    }
                    Some(SchedulerCommand::Pause) => {} // No-op
                    None => {
                        is_running = false; // channel closed
                    }
                }
                continue;
            }

            // 1. Fetch next packet if we don't have one queued/paused
            if current_packet.is_none() {
                let read_res = {
                    let mut src = self.source_port.lock().await;
                    src.read_next_packet()
                };

                match read_res {
                    Ok(Some(pkt)) => {
                        current_packet = Some(pkt);
                    }
                    Ok(None) => {
                        // End of file reached
                        if self.loop_enabled {
                            tracing::info!("Looping playback: resetting to beginning of telemetry file");
                            let mut src = self.source_port.lock().await;
                            // Reset back to start
                            let start_ts = first_packet.as_ref().map(|p| p.original_timestamp_ns).unwrap_or(0);
                            if let Err(e) = src.seek(start_ts) {
                                let _ = self.notify_tx.send(SchedulerNotification::FatalError(
                                    format!("Failed to loop seek: {}", e)
                                )).await;
                                break;
                            }
                            timing_engine.reset(start_ts);
                            continue;
                        } else {
                            let _ = self.notify_tx.send(SchedulerNotification::Eof).await;
                            break;
                        }
                    }
                    Err(e) => {
                        // Classify errors: standard packet headers skipping vs fatal I/O
                        match e {
                            ReplayError::PacketCorruption(ref msg) => {
                                tracing::warn!("Skipping corrupted packet: {}", msg);
                                continue;
                            }
                            _ => {
                                let _ = self.notify_tx.send(SchedulerNotification::FatalError(e.to_string())).await;
                                break;
                            }
                        }
                    }
                }
            }

            // 2. We now have a valid packet. Calculate its delay
            let pkt = current_packet.as_ref().unwrap();
            let delay = match paused_remaining_delay.take() {
                Some(rem) => rem,
                None => timing_engine.compute_delay(pkt.original_timestamp_ns),
            };

            // 3. Sleep unless delay is zero, allowing instant interruption via select
            if delay.is_zero() {
                // Publish immediately
                let pkt_to_pub = current_packet.take().unwrap();
                let envelope = self.envelope_builder.build(&pkt_to_pub);
                let seq = envelope.sequence_number;
                if let Err(e) = self.publish_port.publish(envelope) {
                    let _ = self.notify_tx.send(SchedulerNotification::FatalError(
                        format!("Failed to publish packet: {}", e)
                    )).await;
                    break;
                }
                let apid_str = if let Some(ref ccsds) = pkt_to_pub.ccsds {
                    format!(" (APID: {})", ccsds.apid)
                } else {
                    "".to_string()
                };
                tracing::info!(
                    "Published packet: sequence_number={}, timestamp_ns={}{}",
                    seq,
                    pkt_to_pub.original_timestamp_ns,
                    apid_str
                );
                let apid = pkt_to_pub.ccsds.as_ref().map(|c| c.apid);
                let _ = self.notify_tx.send(SchedulerNotification::PacketPublished {
                    sequence_number: seq,
                    timestamp_ns: pkt_to_pub.original_timestamp_ns,
                    apid,
                }).await;
            } else {
                let sleep_fut = tokio::time::sleep(delay);
                tokio::pin!(sleep_fut);
                let sleep_start = Instant::now();

                tokio::select! {
                    _ = &mut sleep_fut => {
                        // Sleep complete, publish
                        let pkt_to_pub = current_packet.take().unwrap();
                        let envelope = self.envelope_builder.build(&pkt_to_pub);
                        let seq = envelope.sequence_number;
                        if let Err(e) = self.publish_port.publish(envelope) {
                            let _ = self.notify_tx.send(SchedulerNotification::FatalError(
                                format!("Failed to publish packet: {}", e)
                            )).await;
                            break;
                        }
                        let apid_str = if let Some(ref ccsds) = pkt_to_pub.ccsds {
                            format!(" (APID: {})", ccsds.apid)
                        } else {
                            "".to_string()
                        };
                        tracing::info!(
                            "Published packet: sequence_number={}, timestamp_ns={}{}",
                            seq,
                            pkt_to_pub.original_timestamp_ns,
                            apid_str
                        );
                        let apid = pkt_to_pub.ccsds.as_ref().map(|c| c.apid);
                        let _ = self.notify_tx.send(SchedulerNotification::PacketPublished {
                            sequence_number: seq,
                            timestamp_ns: pkt_to_pub.original_timestamp_ns,
                            apid,
                        }).await;
                    }
                    cmd_opt = self.cmd_rx.recv() => {
                        match cmd_opt {
                            Some(SchedulerCommand::Pause) => {
                                is_paused = true;
                                timing_engine.pause();
                                let elapsed = sleep_start.elapsed();
                                let remaining = delay.saturating_sub(elapsed);
                                paused_remaining_delay = Some(remaining);
                            }
                            Some(SchedulerCommand::Resume) => {
                                // No-op, already running
                            }
                            Some(SchedulerCommand::Stop) => {
                                is_running = false;
                            }
                            Some(SchedulerCommand::Seek { timestamp_ns }) => {
                                let mut src = self.source_port.lock().await;
                                if let Err(e) = src.seek(timestamp_ns) {
                                    tracing::error!("Seek failed during playback: {}", e);
                                }
                                timing_engine.reset(timestamp_ns);
                                current_packet = None;
                                paused_remaining_delay = None;
                            }
                            Some(SchedulerCommand::SetSpeed { speed }) => {
                                self.speed = speed;
                                if let Some(ref p) = current_packet {
                                    timing_engine.set_speed(speed, p.original_timestamp_ns);
                                    // Recompute delay with new speed
                                    let new_delay = timing_engine.compute_delay(p.original_timestamp_ns);
                                    paused_remaining_delay = Some(new_delay);
                                }
                            }
                            Some(SchedulerCommand::SetLoop { enabled }) => {
                                self.loop_enabled = enabled;
                            }
                            None => {
                                is_running = false;
                            }
                        }
                    }
                }
            }
        }
    }
}

use std::sync::Arc;
use tonic::{Request, Response, Status};
use crate::api::replay::v1::replay_service_server::ReplayService;
use crate::api::replay::v1::*;
use crate::application::command_handler::CommandHandler;
use crate::domain::commands::ReplayCommand;
use crate::domain::errors::ReplayError;

/// Adapter implementing the gRPC driving/inbound interface.
pub struct ReplayGrpcAdapter {
    command_handler: Arc<CommandHandler>,
}

impl ReplayGrpcAdapter {
    /// Creates a new ReplayGrpcAdapter.
    pub fn new(command_handler: Arc<CommandHandler>) -> Self {
        Self { command_handler }
    }
}

fn map_error(err: ReplayError) -> Status {
    match err {
        ReplayError::Configuration(msg) => Status::invalid_argument(format!("Configuration error: {}", msg)),
        ReplayError::FileIo(msg) => Status::not_found(format!("File/IO error: {}", msg)),
        ReplayError::PacketCorruption(msg) => Status::invalid_argument(format!("Packet corruption: {}", msg)),
        ReplayError::TimestampCorruption(msg) => Status::invalid_argument(format!("Timestamp corruption: {}", msg)),
        ReplayError::InvalidTransition { current, event } => {
            Status::failed_precondition(format!("Invalid transition: cannot run command '{}' when state is '{}'", event, current))
        }
        ReplayError::Network(msg) => Status::unavailable(format!("Network error: {}", msg)),
        ReplayError::Eof => Status::out_of_range("EOF reached"),
    }
}

#[tonic::async_trait]
impl ReplayService for ReplayGrpcAdapter {
    async fn load_file(
        &self,
        request: Request<LoadFileRequest>,
    ) -> Result<Response<LoadFileResponse>, Status> {
        let req = request.into_inner();
        let cmd = ReplayCommand::LoadFile {
            file_path: req.file_path,
            file_type: req.file_type,
            target_stage: req.target_stage,
        };

        match self.command_handler.handle(cmd).await {
            Ok(Some(meta)) => Ok(Response::new(LoadFileResponse {
                success: true,
                message: "File loaded successfully".to_string(),
                total_packets: meta.total_packets,
                duration_ns: meta.duration_ns,
            })),
            Ok(None) => Err(Status::internal("Load command succeeded but returned no metadata")),
            Err(e) => Err(map_error(e)),
        }
    }

    async fn start_playback(
        &self,
        request: Request<StartPlaybackRequest>,
    ) -> Result<Response<StartPlaybackResponse>, Status> {
        let req = request.into_inner();
        let cmd = ReplayCommand::Start {
            speed: req.speed,
            loop_enabled: req.r#loop,
        };

        match self.command_handler.handle(cmd).await {
            Ok(_) => Ok(Response::new(StartPlaybackResponse {
                success: true,
                message: "Playback started successfully".to_string(),
            })),
            Err(e) => Err(map_error(e)),
        }
    }

    async fn pause_playback(
        &self,
        _request: Request<PausePlaybackRequest>,
    ) -> Result<Response<PausePlaybackResponse>, Status> {
        match self.command_handler.handle(ReplayCommand::Pause).await {
            Ok(_) => Ok(Response::new(PausePlaybackResponse { success: true })),
            Err(e) => Err(map_error(e)),
        }
    }

    async fn resume_playback(
        &self,
        _request: Request<ResumePlaybackRequest>,
    ) -> Result<Response<ResumePlaybackResponse>, Status> {
        match self.command_handler.handle(ReplayCommand::Resume).await {
            Ok(_) => Ok(Response::new(ResumePlaybackResponse { success: true })),
            Err(e) => Err(map_error(e)),
        }
    }

    async fn seek_playback(
        &self,
        request: Request<SeekPlaybackRequest>,
    ) -> Result<Response<SeekPlaybackResponse>, Status> {
        let req = request.into_inner();
        let cmd = ReplayCommand::Seek {
            target_timestamp_ns: req.target_timestamp_ns,
        };

        match self.command_handler.handle(cmd).await {
            Ok(_) => Ok(Response::new(SeekPlaybackResponse { success: true })),
            Err(e) => Err(map_error(e)),
        }
    }

    async fn stop_playback(
        &self,
        _request: Request<StopPlaybackRequest>,
    ) -> Result<Response<StopPlaybackResponse>, Status> {
        match self.command_handler.handle(ReplayCommand::Stop).await {
            Ok(_) => Ok(Response::new(StopPlaybackResponse { success: true })),
            Err(e) => Err(map_error(e)),
        }
    }

    async fn get_playback_status(
        &self,
        _request: Request<GetPlaybackStatusRequest>,
    ) -> Result<Response<GetPlaybackStatusResponse>, Status> {
        let (state, speed, progress, packets_published, current_timestamp_ns) =
            self.command_handler.get_status().await;

        Ok(Response::new(GetPlaybackStatusResponse {
            state: state.as_str().to_string(),
            speed,
            progress,
            packets_published,
            current_timestamp_ns,
        }))
    }
}

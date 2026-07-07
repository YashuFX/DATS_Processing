use std::sync::Arc;
use axum::{
    routing::{get, post},
    Router, Json, Extension,
    http::StatusCode,
    response::IntoResponse,
};
use tower_http::cors::{Any, CorsLayer};
use serde::{Deserialize, Serialize};
use crate::application::command_handler::CommandHandler;
use crate::domain::commands::ReplayCommand;
use crate::domain::errors::ReplayError;

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
}

fn map_error(err: ReplayError) -> (StatusCode, Json<ErrorResponse>) {
    match err {
        ReplayError::Configuration(msg) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: "INVALID_REQUEST".to_string(), message: msg }),
        ),
        ReplayError::FileIo(msg) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse { error: "FILE_NOT_FOUND".to_string(), message: msg }),
        ),
        ReplayError::InvalidTransition { current, event } => (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "INVALID_STATE".to_string(),
                message: format!("Cannot run command '{}' when state is '{}'", event, current),
            }),
        ),
        ReplayError::PacketCorruption(msg) | ReplayError::TimestampCorruption(msg) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorResponse { error: "INVALID_FILE".to_string(), message: msg }),
        ),
        ReplayError::Network(msg) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse { error: "UNAVAILABLE".to_string(), message: msg }),
        ),
        ReplayError::Eof => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: "INVALID_REQUEST".to_string(), message: "EOF reached".to_string() }),
        ),
    }
}

// Route Handlers

#[derive(Deserialize)]
pub struct LoadRequest {
    pub file_path: String,
    pub file_type: String,
    pub target_stage: Option<i32>,
}

#[derive(Serialize)]
pub struct FileDetails {
    pub path: String,
    pub size_bytes: u64,
    pub estimated_packets: u64,
    pub estimated_duration_seconds: f64,
    pub file_type: String,
}

#[derive(Serialize)]
pub struct LoadResponse {
    pub status: String,
    pub file: FileDetails,
}

async fn load_file(
    Extension(handler): Extension<Arc<CommandHandler>>,
    Json(payload): Json<LoadRequest>,
) -> impl IntoResponse {
    let target_stage = payload.target_stage.unwrap_or(0);
    let cmd = ReplayCommand::LoadFile {
        file_path: payload.file_path.clone(),
        file_type: payload.file_type.clone(),
        target_stage,
    };

    match handler.handle(cmd).await {
        Ok(Some(meta)) => {
            // Get actual file size if it exists, otherwise return 0
            let size_bytes = std::fs::metadata(&payload.file_path)
                .map(|m| m.len())
                .unwrap_or(0);

            let resp = LoadResponse {
                status: "READY".to_string(),
                file: FileDetails {
                    path: payload.file_path,
                    size_bytes,
                    estimated_packets: meta.total_packets,
                    estimated_duration_seconds: meta.duration_ns as f64 / 1_000_000_000.0,
                    file_type: payload.file_type,
                },
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Ok(None) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "INTERNAL".to_string(),
                message: "Load succeeded but did not return metadata".to_string(),
            }),
        )
            .into_response(),
        Err(e) => {
            let (status, err_resp) = map_error(e);
            (status, err_resp).into_response()
        }
    }
}

#[derive(Deserialize, Default)]
pub struct StartRequest {
    pub speed: Option<f64>,
    pub loop_enabled: Option<bool>,
}

#[derive(Serialize)]
pub struct StartResponse {
    pub status: String,
    pub speed: f64,
    pub started_at: String,
}

async fn start_playback(
    Extension(handler): Extension<Arc<CommandHandler>>,
    payload: Option<Json<StartRequest>>,
) -> impl IntoResponse {
    let p = payload.map(|Json(x)| x).unwrap_or_default();
    let speed = p.speed.unwrap_or(1.0);
    let loop_enabled = p.loop_enabled.unwrap_or(false);

    let cmd = ReplayCommand::Start { speed, loop_enabled };
    match handler.handle(cmd).await {
        Ok(_) => {
            let started_at = chrono::Utc::now().to_rfc3339();
            let resp = StartResponse {
                status: "RUNNING".to_string(),
                speed,
                started_at,
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => {
            let (status, err_resp) = map_error(e);
            (status, err_resp).into_response()
        }
    }
}

#[derive(Serialize)]
pub struct PauseResponse {
    pub status: String,
    pub paused_at_packet: u64,
    pub paused_at_timestamp: u64,
}

async fn pause_playback(
    Extension(handler): Extension<Arc<CommandHandler>>,
) -> impl IntoResponse {
    match handler.handle(ReplayCommand::Pause).await {
        Ok(_) => {
            let (_, _, _, packets, ts) = handler.get_status().await;
            let resp = PauseResponse {
                status: "PAUSED".to_string(),
                paused_at_packet: packets,
                paused_at_timestamp: ts,
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => {
            let (status, err_resp) = map_error(e);
            (status, err_resp).into_response()
        }
    }
}

#[derive(Serialize)]
pub struct ResumeResponse {
    pub status: String,
    pub resumed_at_packet: u64,
}

async fn resume_playback(
    Extension(handler): Extension<Arc<CommandHandler>>,
) -> impl IntoResponse {
    match handler.handle(ReplayCommand::Resume).await {
        Ok(_) => {
            let (_, _, _, packets, _) = handler.get_status().await;
            let resp = ResumeResponse {
                status: "RUNNING".to_string(),
                resumed_at_packet: packets,
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => {
            let (status, err_resp) = map_error(e);
            (status, err_resp).into_response()
        }
    }
}

#[derive(Serialize)]
pub struct StopResponse {
    pub status: String,
}

async fn stop_playback(
    Extension(handler): Extension<Arc<CommandHandler>>,
) -> impl IntoResponse {
    match handler.handle(ReplayCommand::Stop).await {
        Ok(_) => {
            let resp = StopResponse {
                status: "STOPPED".to_string(),
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => {
            let (status, err_resp) = map_error(e);
            (status, err_resp).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct SeekRequest {
    pub target_timestamp: u64,
}

#[derive(Serialize)]
pub struct SeekResponse {
    pub status: String,
    pub seeked_to_packet: u64,
    pub seeked_to_timestamp: u64,
}

async fn seek_playback(
    Extension(handler): Extension<Arc<CommandHandler>>,
    Json(payload): Json<SeekRequest>,
) -> impl IntoResponse {
    let cmd = ReplayCommand::Seek {
        target_timestamp_ns: payload.target_timestamp,
    };
    match handler.handle(cmd).await {
        Ok(_) => {
            let (state, _, _, packets, ts) = handler.get_status().await;
            let resp = SeekResponse {
                status: state.as_str().to_string(),
                seeked_to_packet: packets,
                seeked_to_timestamp: ts,
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => {
            let (status, err_resp) = map_error(e);
            (status, err_resp).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct SpeedRequest {
    pub speed: f64,
}

#[derive(Serialize)]
pub struct SpeedResponse {
    pub speed: f64,
    pub previous_speed: f64,
}

async fn change_speed(
    Extension(handler): Extension<Arc<CommandHandler>>,
    Json(payload): Json<SpeedRequest>,
) -> impl IntoResponse {
    let (_, prev_speed, _, _, _) = handler.get_status().await;
    let cmd = ReplayCommand::SetSpeed { speed: payload.speed };
    match handler.handle(cmd).await {
        Ok(_) => {
            let resp = SpeedResponse {
                speed: payload.speed,
                previous_speed: prev_speed,
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => {
            let (status, err_resp) = map_error(e);
            (status, err_resp).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct LoopRequest {
    pub enabled: bool,
}

#[derive(Serialize)]
pub struct LoopResponse {
    pub loop_enabled: bool,
}

async fn set_loop(
    Extension(handler): Extension<Arc<CommandHandler>>,
    Json(payload): Json<LoopRequest>,
) -> impl IntoResponse {
    let cmd = ReplayCommand::SetLoop { enabled: payload.enabled };
    match handler.handle(cmd).await {
        Ok(_) => {
            let resp = LoopResponse {
                loop_enabled: payload.enabled,
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => {
            let (status, err_resp) = map_error(e);
            (status, err_resp).into_response()
        }
    }
}

#[derive(Serialize)]
pub struct PlaybackProgressDetails {
    pub packets_published: u64,
    pub total_packets_estimated: u64,
    pub progress_percent: f64,
    pub current_timestamp: u64,
}

#[derive(Serialize)]
pub struct StatusPlaybackDetails {
    pub speed: f64,
    pub loop_enabled: bool,
}

#[derive(Serialize)]
pub struct GetStatusResponse {
    pub state: String,
    pub playback: StatusPlaybackDetails,
    pub progress: PlaybackProgressDetails,
}

async fn get_playback_status_rest(
    Extension(handler): Extension<Arc<CommandHandler>>,
) -> impl IntoResponse {
    let (state, speed, progress, packets_published, current_timestamp_ns) =
        handler.get_status().await;

    // Use dummy estimations if no metadata is loaded
    let total_packets_estimated = if packets_published > 0 && progress > 0.0 {
        (packets_published as f64 / progress) as u64
    } else {
        0
    };

    let resp = GetStatusResponse {
        state: state.as_str().to_string(),
        playback: StatusPlaybackDetails {
            speed,
            loop_enabled: false, // loop_enabled is queried or default
        },
        progress: PlaybackProgressDetails {
            packets_published,
            total_packets_estimated,
            progress_percent: progress * 100.0,
            current_timestamp: current_timestamp_ns,
        },
    };

    (StatusCode::OK, Json(resp))
}

async fn get_recent_packets_rest(
    Extension(handler): Extension<Arc<CommandHandler>>,
) -> impl IntoResponse {
    let packets = handler.get_recent_packets().await;
    (StatusCode::OK, Json(packets))
}

// Health Probes

async fn health_live() -> &'static str {
    "OK"
}

async fn health_ready() -> &'static str {
    "OK"
}

async fn health_startup() -> &'static str {
    "OK"
}

/// Creates the Axum REST API Router.
pub fn create_rest_router(handler: Arc<CommandHandler>) -> Router {
    Router::new()
        .route("/api/v1/replay/load", post(load_file))
        .route("/api/v1/replay/start", post(start_playback))
        .route("/api/v1/replay/pause", post(pause_playback))
        .route("/api/v1/replay/resume", post(resume_playback))
        .route("/api/v1/replay/stop", post(stop_playback))
        .route("/api/v1/replay/seek", post(seek_playback))
        .route("/api/v1/replay/speed", post(change_speed))
        .route("/api/v1/replay/loop", post(set_loop))
        .route("/api/v1/replay/status", get(get_playback_status_rest))
        .route("/api/v1/replay/packets", get(get_recent_packets_rest))
        .route("/health/live", get(health_live))
        .route("/health/ready", get(health_ready))
        .route("/health/startup", get(health_startup))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(Extension(handler))
}

use serde::Deserialize;
use std::path::Path;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub server: ServerSettings,
    pub replay: ReplaySettings,
    pub publisher: PublisherSettings,
    pub observability: ObservabilitySettings,
    pub mission: Option<MissionSettings>,
    pub satellite: Option<SatelliteSettings>,
    pub station: Option<StationSettings>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerSettings {
    pub rest: HostPort,
    pub grpc: HostPort,
}

#[derive(Debug, Deserialize, Clone)]
pub struct HostPort {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ReplaySettings {
    pub default_speed: f64,
    pub max_speed: f64,
    pub io_buffer_size_bytes: usize,
    pub drift_correction_enabled: bool,
    pub drift_correction_interval_packets: u64,
    pub max_packet_size_bytes: usize,
    pub file_base_directory: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PublisherSettings {
    pub downstream_address: String,
    pub buffer_size: usize,
    pub retry_attempts: u32,
    pub retry_delay_ms: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ObservabilitySettings {
    pub log_level: String,
    pub log_format: String,
    pub metrics_port: u16,
}

impl Settings {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let settings: Self = serde_yaml::from_str(&content)?;
        Ok(settings)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct MissionSettings {
    pub mission_id: u32,
    pub mission_name: String,
    pub mission_code: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SatelliteSettings {
    pub satellite_id: u32,
    pub satellite_name: String,
    pub norad_id: u32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StationSettings {
    pub station_id: u32,
    pub station_name: String,
    pub station_code: String,
}

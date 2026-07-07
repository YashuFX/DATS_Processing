#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum SourceStatus {
    Registered,
    Connected,
    Disconnected,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SourceRegistration {
    pub source_id: String,
    pub source_type: String, // e.g., "REPLAY", "TCP", "UDP"
    pub source_name: String,
    pub mission_id: u32,
    pub mission_name: String,
    pub mission_code: String,
    pub satellite_id: u32,
    pub satellite_name: String,
    pub norad_id: u32,
    pub station_id: u32,
    pub station_name: String,
    pub station_code: String,
    pub registered_at: u64,
    pub status: SourceStatus,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum SessionStatus {
    Active,
    Completed,
    Failed,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub session_id: String,
    pub source_id: String,
    pub started_at: u64, // unix timestamp in ms
    pub status: SessionStatus,
    pub packets_received: u64,
    pub packets_published: u64,
    pub packets_rejected: u64,
    pub last_packet_at: u64,
}

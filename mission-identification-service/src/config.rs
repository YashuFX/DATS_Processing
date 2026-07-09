use std::env;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub amqp_url: String,
    pub registry_file_path: String,
    pub source_exchange: String,
    pub source_queue: String,
    pub source_routing_key: String,
    pub destination_exchange: String,
    pub prefetch_count: u16,
    pub publish_timeout_ms: u64,
    pub retry_max_attempts: u32,
    pub metrics_port: u16,
    pub consumer_tag: String,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, String> {
        let amqp_url = env::var("AMQP_URL")
            .unwrap_or_else(|_| "amqp://guest:guest@localhost:5672/%2f".to_string());
        
        let registry_file_path = env::var("REGISTRY_FILE_PATH")
            .unwrap_or_else(|_| "configs/registry.yaml".to_string());

        let source_exchange = env::var("SOURCE_EXCHANGE")
            .unwrap_or_else(|_| "telemetry.decoded".to_string());

        let source_queue = env::var("SOURCE_QUEUE")
            .unwrap_or_else(|_| "mission.identify".to_string());

        let source_routing_key = env::var("SOURCE_ROUTING_KEY")
            .unwrap_or_else(|_| "#.decoded".to_string());

        let destination_exchange = env::var("DESTINATION_EXCHANGE")
            .unwrap_or_else(|_| "telemetry.identified".to_string());

        let prefetch_count = env::var("PREFETCH_COUNT")
            .unwrap_or_else(|_| "50".to_string())
            .parse::<u16>()
            .map_err(|e| format!("Invalid PREFETCH_COUNT: {}", e))?;

        let publish_timeout_ms = env::var("PUBLISH_TIMEOUT_MS")
            .unwrap_or_else(|_| "5000".to_string())
            .parse::<u64>()
            .map_err(|e| format!("Invalid PUBLISH_TIMEOUT_MS: {}", e))?;

        let retry_max_attempts = env::var("RETRY_MAX_ATTEMPTS")
            .unwrap_or_else(|_| "5".to_string())
            .parse::<u32>()
            .map_err(|e| format!("Invalid RETRY_MAX_ATTEMPTS: {}", e))?;

        let metrics_port = env::var("METRICS_PORT")
            .unwrap_or_else(|_| "8083".to_string())
            .parse::<u16>()
            .map_err(|e| format!("Invalid METRICS_PORT: {}", e))?;

        let consumer_tag = env::var("CONSUMER_TAG")
            .unwrap_or_else(|_| "mission-identification-service-1".to_string());

        Ok(Self {
            amqp_url,
            registry_file_path,
            source_exchange,
            source_queue,
            source_routing_key,
            destination_exchange,
            prefetch_count,
            publish_timeout_ms,
            retry_max_attempts,
            metrics_port,
            consumer_tag,
        })
    }
}

use std::env;
use crate::domain::errors::XtceError;

#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Full AMQP connection URI.
    pub amqp_url: String,

    /// Exchange from which identified telemetry envelopes are consumed.
    pub source_exchange: String,

    /// Name of the queue bound to the source exchange.
    pub source_queue: String,

    /// Routing key pattern for the consumer binding.
    pub source_routing_key: String,

    /// Unique consumer identifier.
    pub consumer_tag: String,

    /// QoS prefetch limit.
    pub prefetch_count: u16,

    /// Exchange to publish decommutated engineering parameters to.
    pub destination_exchange: String,

    /// Directory containing XTCE XML files.
    pub xtce_db_dir: String,

    /// Timeout (in ms) waiting for publisher confirmations.
    pub publish_timeout_ms: u64,

    /// Maximum retry attempts on publish.
    pub retry_max_attempts: usize,
}

impl AppConfig {
    /// Load configuration entirely from environment variables.
    pub fn from_env() -> Result<Self, XtceError> {
        let amqp_url = env::var("AMQP_URL").map_err(|_| {
            XtceError::ConfigError(
                "AMQP_URL environment variable is required but not set.".to_string(),
            )
        })?;

        if amqp_url.trim().is_empty() {
            return Err(XtceError::ConfigError(
                "AMQP_URL is set but empty".to_string(),
            ));
        }

        let source_exchange = env::var("SOURCE_EXCHANGE")
            .unwrap_or_else(|_| "telemetry.identified".to_string());

        let source_queue = env::var("SOURCE_QUEUE")
            .unwrap_or_else(|_| "xtce.process".to_string());

        let source_routing_key = env::var("SOURCE_ROUTING_KEY")
            .unwrap_or_else(|_| "#.identified".to_string());

        let consumer_tag = env::var("CONSUMER_TAG")
            .unwrap_or_else(|_| "xtce-decoder-1".to_string());

        let prefetch_count = env::var("PREFETCH_COUNT")
            .unwrap_or_else(|_| "50".to_string())
            .parse::<u16>()
            .map_err(|e| {
                XtceError::ConfigError(format!("PREFETCH_COUNT must be a valid u16 integer: {e}"))
            })?;

        let destination_exchange = env::var("DESTINATION_EXCHANGE")
            .unwrap_or_else(|_| "telemetry.engineering".to_string());

        let xtce_db_dir = env::var("XTCE_DB_DIR")
            .unwrap_or_else(|_| "/etc/must/xtce".to_string());

        let publish_timeout_ms = env::var("PUBLISH_TIMEOUT_MS")
            .unwrap_or_else(|_| "5000".to_string())
            .parse::<u64>()
            .map_err(|e| {
                XtceError::ConfigError(format!("PUBLISH_TIMEOUT_MS must be a valid u64 integer: {e}"))
            })?;

        let retry_max_attempts = env::var("RETRY_MAX_ATTEMPTS")
            .unwrap_or_else(|_| "5".to_string())
            .parse::<usize>()
            .map_err(|e| {
                XtceError::ConfigError(format!("RETRY_MAX_ATTEMPTS must be a valid usize: {e}"))
            })?;

        Ok(AppConfig {
            amqp_url,
            source_exchange,
            source_queue,
            source_routing_key,
            consumer_tag,
            prefetch_count,
            destination_exchange,
            xtce_db_dir,
            publish_timeout_ms,
            retry_max_attempts,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_config_missing_amqp_url() {
        let _guard = ENV_MUTEX.lock().unwrap();
        env::remove_var("AMQP_URL");
        let err = AppConfig::from_env().unwrap_err();
        assert!(matches!(err, XtceError::ConfigError(_)));
    }

    #[test]
    fn test_config_defaults() {
        let _guard = ENV_MUTEX.lock().unwrap();
        env::set_var("AMQP_URL", "amqp://guest:guest@localhost:5672/%2f");
        env::remove_var("SOURCE_EXCHANGE");
        env::remove_var("SOURCE_QUEUE");
        env::remove_var("SOURCE_ROUTING_KEY");
        env::remove_var("CONSUMER_TAG");
        env::remove_var("PREFETCH_COUNT");
        env::remove_var("DESTINATION_EXCHANGE");
        env::remove_var("XTCE_DB_DIR");
        env::remove_var("PUBLISH_TIMEOUT_MS");
        env::remove_var("RETRY_MAX_ATTEMPTS");

        let cfg = AppConfig::from_env().unwrap();
        assert_eq!(cfg.source_exchange, "telemetry.identified");
        assert_eq!(cfg.source_queue, "xtce.process");
        assert_eq!(cfg.source_routing_key, "#.identified");
        assert_eq!(cfg.consumer_tag, "xtce-decoder-1");
        assert_eq!(cfg.prefetch_count, 50);
        assert_eq!(cfg.destination_exchange, "telemetry.engineering");
        assert_eq!(cfg.xtce_db_dir, "/etc/must/xtce");
        assert_eq!(cfg.publish_timeout_ms, 5000);
        assert_eq!(cfg.retry_max_attempts, 5);

        env::remove_var("AMQP_URL");
    }
}

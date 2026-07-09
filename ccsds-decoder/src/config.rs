// ── Service Configuration ─────────────────────────────────────────────────────
//
// All settings are loaded from environment variables.
// Nothing is hard-coded.
//
// Required:
//   AMQP_URL                 — AMQP connection string (fail-fast if absent)
//
// Optional (sensible defaults for local dev):
//   SOURCE_EXCHANGE          — Exchange to consume from       (default: "telemetry.raw")
//   SOURCE_QUEUE             — Queue to bind and consume      (default: "ccsds-decoder.raw")
//   SOURCE_ROUTING_KEY       — Binding routing key            (default: "#")
//   CONSUMER_TAG             — AMQP consumer tag              (default: "ccsds-decoder-1")
//   PREFETCH_COUNT           — QoS prefetch window            (default: 10)
//   CHECK_CRC                — Enable CRC-16 validation       (default: false)
//   DESTINATION_EXCHANGE     — Exchange to publish to         (default: "telemetry.decoded")
//   PUBLISH_TIMEOUT_MS       — Timeout waiting for confirm    (default: 5000)
//   RETRY_MAX_ATTEMPTS       — Max retry attempts on publish  (default: 5)
//
// Why CHECK_CRC is configurable:
//   Some missions do not append CRC to their space packets. Making it an env
//   var means the same binary serves all missions without recompilation.

use std::env;

use crate::domain::errors::DecoderError;

#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Full AMQP URI, e.g. "amqp://guest:guest@localhost:5672/%2f"
    pub amqp_url: String,

    /// Name of the exchange from which raw telemetry is consumed.
    pub source_exchange: String,

    /// Name of the durable queue bound to `source_exchange`.
    pub source_queue: String,

    /// Routing key used when binding the queue to the exchange.
    pub source_routing_key: String,

    /// Unique consumer tag sent to the broker.
    pub consumer_tag: String,

    /// AMQP QoS prefetch count — how many unacked messages the broker may
    /// send before pausing delivery. Set to 1 for strict ordering, higher
    /// for throughput.
    pub prefetch_count: u16,

    /// When true the orchestrator calls `validate_crc()` on every packet.
    /// Set to false for missions that do not append CRC bytes.
    pub check_crc: bool,

    /// Name of the exchange to publish decoded envelopes to.
    pub destination_exchange: String,

    /// Timeout (in milliseconds) waiting for publisher confirms.
    pub publish_timeout_ms: u64,

    /// Maximum retry attempts for transient publishing errors.
    pub retry_max_attempts: usize,
}

impl AppConfig {
    /// Load configuration entirely from environment variables.
    ///
    /// Returns `DecoderError::ConfigError` if `AMQP_URL` is absent.
    /// All other fields fall back to safe defaults if their env var is unset.
    pub fn from_env() -> Result<Self, DecoderError> {
        let amqp_url = env::var("AMQP_URL").map_err(|_| {
            DecoderError::ConfigError(
                "AMQP_URL environment variable is required but not set. \
                 Example: AMQP_URL=amqp://guest:guest@localhost:5672/%2f"
                    .to_string(),
            )
        })?;

        if amqp_url.trim().is_empty() {
            return Err(DecoderError::ConfigError(
                "AMQP_URL is set but empty".to_string(),
            ));
        }

        let source_exchange =
            env::var("SOURCE_EXCHANGE").unwrap_or_else(|_| "telemetry.raw".to_string());

        let source_queue =
            env::var("SOURCE_QUEUE").unwrap_or_else(|_| "ccsds-decoder.raw".to_string());

        let source_routing_key = env::var("SOURCE_ROUTING_KEY").unwrap_or_else(|_| "#".to_string());

        let consumer_tag =
            env::var("CONSUMER_TAG").unwrap_or_else(|_| "ccsds-decoder-1".to_string());

        let prefetch_count = env::var("PREFETCH_COUNT")
            .unwrap_or_else(|_| "10".to_string())
            .parse::<u16>()
            .map_err(|e| {
                DecoderError::ConfigError(format!(
                    "PREFETCH_COUNT must be a valid u16 integer: {e}"
                ))
            })?;

        let check_crc = env::var("CHECK_CRC")
            .unwrap_or_else(|_| "false".to_string())
            .to_lowercase();

        let check_crc = match check_crc.as_str() {
            "true" | "1" | "yes" => true,
            "false" | "0" | "no" => false,
            other => {
                return Err(DecoderError::ConfigError(format!(
                    "CHECK_CRC must be true/false/1/0/yes/no, got: {other}"
                )))
            }
        };

        let destination_exchange =
            env::var("DESTINATION_EXCHANGE").unwrap_or_else(|_| "telemetry.decoded".to_string());

        let publish_timeout_ms = env::var("PUBLISH_TIMEOUT_MS")
            .unwrap_or_else(|_| "5000".to_string())
            .parse::<u64>()
            .map_err(|e| {
                DecoderError::ConfigError(format!(
                    "PUBLISH_TIMEOUT_MS must be a valid u64 integer: {e}"
                ))
            })?;

        let retry_max_attempts = env::var("RETRY_MAX_ATTEMPTS")
            .unwrap_or_else(|_| "5".to_string())
            .parse::<usize>()
            .map_err(|e| {
                DecoderError::ConfigError(format!(
                    "RETRY_MAX_ATTEMPTS must be a valid usize integer: {e}"
                ))
            })?;

        Ok(AppConfig {
            amqp_url,
            source_exchange,
            source_queue,
            source_routing_key,
            consumer_tag,
            prefetch_count,
            check_crc,
            destination_exchange,
            publish_timeout_ms,
            retry_max_attempts,
        })
    }
}

// ── Unit Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_config_missing_amqp_url() {
        let _guard = ENV_MUTEX.lock().unwrap();
        // Ensure AMQP_URL is unset for this test
        env::remove_var("AMQP_URL");
        let err = AppConfig::from_env().unwrap_err();
        assert!(matches!(err, DecoderError::ConfigError(_)));
    }

    #[test]
    fn test_config_empty_amqp_url() {
        let _guard = ENV_MUTEX.lock().unwrap();
        env::set_var("AMQP_URL", "");
        let err = AppConfig::from_env().unwrap_err();
        assert!(matches!(err, DecoderError::ConfigError(_)));
        env::remove_var("AMQP_URL");
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
        env::remove_var("CHECK_CRC");
        env::remove_var("DESTINATION_EXCHANGE");
        env::remove_var("PUBLISH_TIMEOUT_MS");
        env::remove_var("RETRY_MAX_ATTEMPTS");

        let cfg = AppConfig::from_env().unwrap();
        assert_eq!(cfg.source_exchange, "telemetry.raw");
        assert_eq!(cfg.source_queue, "ccsds-decoder.raw");
        assert_eq!(cfg.source_routing_key, "#");
        assert_eq!(cfg.consumer_tag, "ccsds-decoder-1");
        assert_eq!(cfg.prefetch_count, 10);
        assert!(!cfg.check_crc);
        assert_eq!(cfg.destination_exchange, "telemetry.decoded");
        assert_eq!(cfg.publish_timeout_ms, 5000);
        assert_eq!(cfg.retry_max_attempts, 5);

        env::remove_var("AMQP_URL");
    }

    #[test]
    fn test_config_check_crc_true() {
        let _guard = ENV_MUTEX.lock().unwrap();
        env::set_var("AMQP_URL", "amqp://guest:guest@localhost:5672/%2f");
        env::set_var("CHECK_CRC", "true");
        let cfg = AppConfig::from_env().unwrap();
        assert!(cfg.check_crc);
        env::remove_var("AMQP_URL");
        env::remove_var("CHECK_CRC");
    }

    #[test]
    fn test_config_invalid_prefetch() {
        let _guard = ENV_MUTEX.lock().unwrap();
        env::set_var("AMQP_URL", "amqp://guest:guest@localhost:5672/%2f");
        env::set_var("PREFETCH_COUNT", "not_a_number");
        let err = AppConfig::from_env().unwrap_err();
        assert!(matches!(err, DecoderError::ConfigError(_)));
        env::remove_var("AMQP_URL");
        env::remove_var("PREFETCH_COUNT");
    }

    #[test]
    fn test_config_custom_outbound() {
        let _guard = ENV_MUTEX.lock().unwrap();
        env::set_var("AMQP_URL", "amqp://guest:guest@localhost:5672/%2f");
        env::set_var("DESTINATION_EXCHANGE", "custom.decoded");
        env::set_var("PUBLISH_TIMEOUT_MS", "3000");
        env::set_var("RETRY_MAX_ATTEMPTS", "10");

        let cfg = AppConfig::from_env().unwrap();
        assert_eq!(cfg.destination_exchange, "custom.decoded");
        assert_eq!(cfg.publish_timeout_ms, 3000);
        assert_eq!(cfg.retry_max_attempts, 10);

        env::remove_var("AMQP_URL");
        env::remove_var("DESTINATION_EXCHANGE");
        env::remove_var("PUBLISH_TIMEOUT_MS");
        env::remove_var("RETRY_MAX_ATTEMPTS");
    }
}

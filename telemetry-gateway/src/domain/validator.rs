use crate::api::telemetry::v1::TelemetryEnvelope;
use crate::domain::errors::GatewayError;

pub struct Validator;

impl Validator {
    pub fn validate(
        envelope: &TelemetryEnvelope,
        is_source_registered: bool,
        is_session_active: bool,
    ) -> Result<(), GatewayError> {
        // 1. Non-empty payload validation
        let raw_packet = match &envelope.raw_packet {
            Some(rp) => rp,
            None => {
                return Err(GatewayError::ValidationError("Missing raw packet details".to_string()));
            }
        };

        if raw_packet.data.is_empty() {
            return Err(GatewayError::ValidationError("Empty raw packet data payload".to_string()));
        }

        // 2. Original timestamp validation
        let original_ts = match &envelope.original_timestamp {
            Some(ts) => ts,
            None => {
                return Err(GatewayError::ValidationError("Missing original timestamp".to_string()));
            }
        };

        if original_ts.nanos_since_epoch == 0 {
            return Err(GatewayError::ValidationError("Original timestamp cannot be zero".to_string()));
        }

        // 3. Source registration check
        if !is_source_registered {
            return Err(GatewayError::ValidationError("Source is not registered".to_string()));
        }

        // 4. Session active check
        if !is_session_active {
            return Err(GatewayError::ValidationError("Session is not active".to_string()));
        }

        Ok(())
    }
}

use crate::api::telemetry::v1::TelemetryEnvelope;

pub struct Router;

impl Router {
    pub fn build_routing_key(envelope: &TelemetryEnvelope) -> String {
        let mission_code = envelope
            .mission
            .as_ref()
            .map(|m| m.mission_code.as_str())
            .unwrap_or("unknown")
            .to_ascii_lowercase();

        let satellite_id = envelope
            .satellite
            .as_ref()
            .map(|s| s.satellite_id)
            .unwrap_or(0);

        let apid = envelope.apid;

        format!("{}.sat{}.{}.raw", mission_code, satellite_id, apid)
    }
}

use crate::api::telemetry::v1::{TelemetryEnvelope, ProcessingStage};
use crate::api::common::v1::{MustTimestamp, TimestampSource, MissionIdentifier, SatelliteIdentifier, GroundStationIdentifier};
use crate::domain::models::SourceRegistration;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Enricher;

impl Enricher {
    pub fn enrich(
        envelope: &mut TelemetryEnvelope,
        reg: &SourceRegistration,
        gateway_seq: u64,
    ) {
        let now_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        // 1. Authoritative receive timestamp from gateway clock
        envelope.receive_timestamp = Some(MustTimestamp {
            nanos_since_epoch: now_nanos,
            source: TimestampSource::System as i32,
        });

        // 2. Set processing stage to RAW
        envelope.stage = ProcessingStage::Raw as i32;

        // 3. Resolve metadata fields from source registration config if missing
        if envelope.mission.is_none() {
            envelope.mission = Some(MissionIdentifier {
                mission_id: reg.mission_id,
                mission_name: reg.mission_name.clone(),
                mission_code: reg.mission_code.clone(),
            });
        }

        if envelope.satellite.is_none() {
            envelope.satellite = Some(SatelliteIdentifier {
                satellite_id: reg.satellite_id,
                satellite_name: reg.satellite_name.clone(),
                norad_id: reg.norad_id,
            });
        }

        if envelope.station.is_none() {
            envelope.station = Some(GroundStationIdentifier {
                station_id: reg.station_id,
                station_name: reg.station_name.clone(),
                station_code: reg.station_code.clone(),
            });
        }

        // 4. Populate envelope ID if not already set
        if envelope.envelope_id.is_empty() {
            envelope.envelope_id = uuid::Uuid::new_v4().to_string();
        }

        // 5. Monotonic sequence number
        envelope.sequence_number = gateway_seq;
    }

    pub fn set_publish_timestamp(envelope: &mut TelemetryEnvelope) {
        let now_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        envelope.publish_timestamp = Some(MustTimestamp {
            nanos_since_epoch: now_nanos,
            source: TimestampSource::System as i32,
        });
    }
}

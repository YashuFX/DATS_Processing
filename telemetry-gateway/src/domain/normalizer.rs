use crate::api::telemetry::v1::{TelemetryEnvelope, QualityIndicator};

pub struct Normalizer;

impl Normalizer {
    pub fn normalize(envelope: &mut TelemetryEnvelope) {
        // Normalize whitespaces and codes in identifiers
        if let Some(source) = &mut envelope.source {
            source.source_id = source.source_id.trim().to_string();
            source.source_name = source.source_name.trim().to_string();
        }

        if let Some(mission) = &mut envelope.mission {
            mission.mission_name = mission.mission_name.trim().to_string();
            mission.mission_code = mission.mission_code.trim().to_ascii_lowercase();
        }

        if let Some(satellite) = &mut envelope.satellite {
            satellite.satellite_name = satellite.satellite_name.trim().to_string();
        }

        if let Some(station) = &mut envelope.station {
            station.station_name = station.station_name.trim().to_string();
            station.station_code = station.station_code.trim().to_ascii_lowercase();
        }

        // Standardize quality flags structure
        if envelope.quality.is_none() {
            envelope.quality = Some(QualityIndicator {
                is_valid: true,
                crc_ok: true,
                timestamp_monotonic: true,
                sequence_continuous: true,
                warnings: Vec::new(),
            });
        }
    }
}

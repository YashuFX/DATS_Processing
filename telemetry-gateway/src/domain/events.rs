use crate::api::events::v1::{PlatformEvent, EventSeverity};
use crate::api::common::v1::{MustTimestamp, TimestampSource, SourceIdentifier};
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::HashMap;

pub struct EventBuilder;

impl EventBuilder {
    pub fn build(
        source_id: &str,
        source_name: &str,
        source_type: i32,
        severity: EventSeverity,
        event_type: &str,
        message: &str,
        metadata: HashMap<String, String>,
    ) -> PlatformEvent {
        let now_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        PlatformEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            timestamp: Some(MustTimestamp {
                nanos_since_epoch: now_nanos,
                source: TimestampSource::System as i32,
            }),
            source: Some(SourceIdentifier {
                source_id: source_id.to_string(),
                source_type,
                source_name: source_name.to_string(),
            }),
            severity: severity as i32,
            event_type: event_type.to_string(),
            message: message.to_string(),
            metadata,
        }
    }
}

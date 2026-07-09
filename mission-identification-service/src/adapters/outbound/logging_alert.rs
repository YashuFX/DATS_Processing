use async_trait::async_trait;
use crate::ports::outbound::AlertPort;

pub struct LoggingAlertAdapter;

impl LoggingAlertAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AlertPort for LoggingAlertAdapter {
    async fn alert_unidentified(&self, source_id: &str, apid: u32, vcid: Option<u32>) {
        tracing::warn!(
            event = "unidentified_packet",
            source_id = %source_id,
            apid = %apid,
            vcid = ?vcid,
            "ALERT: Incoming packet matches no registered mission routing rules"
        );
    }

    async fn alert_ambiguous(&self, source_id: &str, apid: u32) {
        tracing::error!(
            event = "ambiguous_packet_rules",
            source_id = %source_id,
            apid = %apid,
            "ALERT: Incoming packet matches multiple ambiguous rules with identical specificity"
        );
    }
}

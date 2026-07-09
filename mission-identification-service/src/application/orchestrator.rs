use std::sync::Arc;
use crate::domain::lookup::RuleLookupEngine;
use crate::domain::errors::DomainError;
use crate::ports::inbound::DeliveryAcker;
use crate::ports::outbound::{IdentifiedPublisher, AlertPort};
use crate::proto::{TelemetryEnvelope, ProcessingStage, MustTimestamp};

pub struct IdentificationOrchestrator {
    lookup_engine: Arc<RuleLookupEngine>,
    publisher: Arc<dyn IdentifiedPublisher>,
    alert_port: Arc<dyn AlertPort>,
}

impl IdentificationOrchestrator {
    pub fn new(
        lookup_engine: Arc<RuleLookupEngine>,
        publisher: Arc<dyn IdentifiedPublisher>,
        alert_port: Arc<dyn AlertPort>,
    ) -> Self {
        Self {
            lookup_engine,
            publisher,
            alert_port,
        }
    }

    pub async fn on_envelope_consumed(
        &self,
        raw_bytes: Vec<u8>,
        _routing_key: String,
        acker: DeliveryAcker,
    ) {
        let start_time = tokio::time::Instant::now();

        // 1. Deserialize envelope
        let mut envelope = match <TelemetryEnvelope as prost::Message>::decode(&raw_bytes[..]) {
            Ok(env) => env,
            Err(e) => {
                tracing::error!("Failed to deserialize TelemetryEnvelope: {}", e);
                acker.nack().await;
                return;
            }
        };

        // 2. Extract packet identifiers for lookup
        let source_id = envelope
            .source
            .as_ref()
            .map(|s| s.source_id.as_str())
            .unwrap_or("unknown");

        let apid = envelope.apid;
        let vcid = if envelope.vcid > 0 { Some(envelope.vcid) } else { None };

        // 3. Resolve Mission & Satellite context
        match self.lookup_engine.resolve(source_id, apid, vcid) {
            Ok(match_result) => {
                // 4. Enrich Envelope in-place
                envelope.mission = Some(match_result.mission.clone());
                envelope.satellite = Some(match_result.satellite.clone());
                envelope.stage = ProcessingStage::Identified as i32;

                // Update publish timestamp
                if let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                    envelope.publish_timestamp = Some(MustTimestamp {
                        nanos_since_epoch: duration.as_nanos() as u64,
                        source: 4, // TIMESTAMP_SOURCE_SYSTEM
                    });
                }

                // Construct outbound routing key: {mission_code}.sat{satellite_id}.{apid}.identified
                let mission_code = match_result.mission.mission_code.to_lowercase();
                let sat_id = match_result.satellite.satellite_id;
                let outbound_routing_key = format!("{}.sat{}.{}.identified", mission_code, sat_id, apid);

                // Publish enriched envelope
                match self.publisher.publish(&envelope, &outbound_routing_key).await {
                    Ok(_) => {
                        tracing::debug!(
                            "Successfully identified and published envelope {}. Mission: {}, Sat: {}",
                            envelope.envelope_id,
                            mission_code,
                            sat_id
                        );
                        
                        // Increment operational success metrics
                        metrics::counter!(
                            "must_mis_packets_processed_total",
                            "mission" => mission_code.clone(),
                            "satellite_id" => sat_id.to_string()
                        ).increment(1);

                        metrics::histogram!("must_mis_lookup_latency_seconds").record(start_time.elapsed().as_secs_f64());

                        acker.ack().await;
                    }
                    Err(e) => {
                        tracing::error!("Outbound publishing failed for envelope {}: {}", envelope.envelope_id, e);
                        acker.nack().await;
                    }
                }
            }
            Err(err) => {
                match err {
                    DomainError::UnidentifiedPacket { ref source_id, apid, vcid } => {
                        tracing::warn!("Unidentified packet from source: '{}', APID: {}, VCID: {:?}", source_id, apid, vcid);
                        
                        metrics::counter!(
                            "must_mis_packets_unidentified_total",
                            "source_id" => source_id.clone(),
                            "apid" => apid.to_string()
                        ).increment(1);

                        self.alert_port.alert_unidentified(source_id, apid, vcid).await;
                    }
                    DomainError::AmbiguousMatch { ref source_id, apid } => {
                        tracing::warn!("Ambiguous rule match for source: '{}', APID: {}", source_id, apid);
                        
                        metrics::counter!(
                            "must_mis_packets_ambiguous_total",
                            "source_id" => source_id.clone(),
                            "apid" => apid.to_string()
                        ).increment(1);

                        self.alert_port.alert_ambiguous(source_id, apid).await;
                    }
                    _ => {}
                }
                
                metrics::histogram!("must_mis_lookup_latency_seconds").record(start_time.elapsed().as_secs_f64());
                
                // Send failed packet to DLQ
                acker.nack().await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::registry::MissionRegistry;
    use crate::ports::inbound::AckerInner;
    use crate::proto::must::common::v1::SourceIdentifier;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use prost::Message;

    struct TestAcker {
        acked: Arc<Mutex<bool>>,
        nacked: Arc<Mutex<bool>>,
    }

    #[async_trait]
    impl AckerInner for TestAcker {
        async fn ack(&mut self) {
            *self.acked.lock().unwrap() = true;
        }
        async fn nack(&mut self) {
            *self.nacked.lock().unwrap() = true;
        }
    }

    struct MockPublisher {
        published: Arc<Mutex<Vec<(TelemetryEnvelope, String)>>>,
    }

    #[async_trait]
    impl IdentifiedPublisher for MockPublisher {
        async fn publish(&self, envelope: &TelemetryEnvelope, routing_key: &str) -> Result<(), DomainError> {
            self.published.lock().unwrap().push((envelope.clone(), routing_key.to_string()));
            Ok(())
        }
    }

    struct MockAlertPort {
        unidentified: Arc<Mutex<Vec<(String, u32, Option<u32>)>>>,
        ambiguous: Arc<Mutex<Vec<(String, u32)>>>,
    }

    #[async_trait]
    impl AlertPort for MockAlertPort {
        async fn alert_unidentified(&self, source_id: &str, apid: u32, vcid: Option<u32>) {
            self.unidentified.lock().unwrap().push((source_id.to_string(), apid, vcid));
        }
        async fn alert_ambiguous(&self, source_id: &str, apid: u32) {
            self.ambiguous.lock().unwrap().push((source_id.to_string(), apid));
        }
    }

    const TEST_YAML: &str = r#"
missions:
  - id: 1
    name: "Chandrayaan-3"
    code: "cy3"
    satellites:
      - id: 101
        name: "Propulsion Module"
        norad_id: 57320
        rules:
          - source_id: "rss-replay"
            apids: [42]
"#;

    #[tokio::test]
    async fn test_orchestrator_success_path() {
        let registry = MissionRegistry::from_yaml(TEST_YAML).unwrap();
        let lookup_engine = Arc::new(RuleLookupEngine::new(registry));

        let published = Arc::new(Mutex::new(Vec::new()));
        let publisher = Arc::new(MockPublisher { published: published.clone() });

        let unidentified = Arc::new(Mutex::new(Vec::new()));
        let ambiguous = Arc::new(Mutex::new(Vec::new()));
        let alert_port = Arc::new(MockAlertPort {
            unidentified: unidentified.clone(),
            ambiguous: ambiguous.clone(),
        });

        let orch = IdentificationOrchestrator::new(lookup_engine, publisher, alert_port);

        // Setup test envelope
        let envelope = TelemetryEnvelope {
            envelope_id: "test-uuid".to_string(),
            sequence_number: 1,
            source: Some(SourceIdentifier {
                source_id: "rss-replay".to_string(),
                source_type: 1,
                source_name: "Replay".to_string(),
            }),
            apid: 42,
            ..Default::default()
        };

        let mut buf = Vec::new();
        envelope.encode(&mut buf).unwrap();

        let acked = Arc::new(Mutex::new(false));
        let nacked = Arc::new(Mutex::new(false));
        let acker = DeliveryAcker::new(Box::new(TestAcker {
            acked: acked.clone(),
            nacked: nacked.clone(),
        }));

        orch.on_envelope_consumed(buf, "unk.sat0.0042.decoded".to_string(), acker).await;

        assert!(*acked.lock().unwrap());
        assert!(!*nacked.lock().unwrap());

        let pub_list = published.lock().unwrap();
        assert_eq!(pub_list.len(), 1);
        let (pub_env, pub_key) = &pub_list[0];
        assert_eq!(pub_key, "cy3.sat101.42.identified");
        assert_eq!(pub_env.stage, ProcessingStage::Identified as i32);
        assert_eq!(pub_env.mission.as_ref().unwrap().mission_code, "cy3");
        assert_eq!(pub_env.satellite.as_ref().unwrap().satellite_id, 101);
    }

    #[tokio::test]
    async fn test_orchestrator_unidentified_path() {
        let registry = MissionRegistry::from_yaml(TEST_YAML).unwrap();
        let lookup_engine = Arc::new(RuleLookupEngine::new(registry));

        let published = Arc::new(Mutex::new(Vec::new()));
        let publisher = Arc::new(MockPublisher { published: published.clone() });

        let unidentified = Arc::new(Mutex::new(Vec::new()));
        let ambiguous = Arc::new(Mutex::new(Vec::new()));
        let alert_port = Arc::new(MockAlertPort {
            unidentified: unidentified.clone(),
            ambiguous: ambiguous.clone(),
        });

        let orch = IdentificationOrchestrator::new(lookup_engine, publisher, alert_port);

        // APID 999 (unregistered)
        let envelope = TelemetryEnvelope {
            envelope_id: "test-uuid-2".to_string(),
            sequence_number: 2,
            source: Some(SourceIdentifier {
                source_id: "rss-replay".to_string(),
                source_type: 1,
                source_name: "Replay".to_string(),
            }),
            apid: 999,
            ..Default::default()
        };

        let mut buf = Vec::new();
        envelope.encode(&mut buf).unwrap();

        let acked = Arc::new(Mutex::new(false));
        let nacked = Arc::new(Mutex::new(false));
        let acker = DeliveryAcker::new(Box::new(TestAcker {
            acked: acked.clone(),
            nacked: nacked.clone(),
        }));

        orch.on_envelope_consumed(buf, "unk.sat0.0999.decoded".to_string(), acker).await;

        assert!(!*acked.lock().unwrap());
        assert!(*nacked.lock().unwrap()); // Should NACK to route to DLQ

        assert_eq!(published.lock().unwrap().len(), 0);
        assert_eq!(unidentified.lock().unwrap().len(), 1);
        assert_eq!(unidentified.lock().unwrap()[0].0, "rss-replay");
        assert_eq!(unidentified.lock().unwrap()[0].1, 999);
    }
}

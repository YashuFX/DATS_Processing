use std::sync::Arc;
use prost::Message;
use crate::config::AppConfig;
use crate::domain::errors::DomainError;
use crate::domain::registry::FormulaRegistry;
use crate::domain::computation::ComputationEngine;
use crate::ports::inbound::DeliveryAcker;
use crate::ports::outbound::{AlertPort, EngineeringPublisher};
use crate::proto::{
    TelemetryEnvelope, TelemetryParameter, ParameterValue,
    parameter_value::Value as ProtoValue, ParameterValidity, ProcessingStage,
};

pub struct ConversionOrchestrator {
    _config: Arc<AppConfig>,
    registry: Arc<FormulaRegistry>,
    publisher: Arc<dyn EngineeringPublisher>,
    alert_port: Arc<dyn AlertPort>,
}

impl ConversionOrchestrator {
    pub fn new(
        config: Arc<AppConfig>,
        registry: Arc<FormulaRegistry>,
        publisher: Arc<dyn EngineeringPublisher>,
        alert_port: Arc<dyn AlertPort>,
    ) -> Self {
        Self {
            _config: config,
            registry,
            publisher,
            alert_port,
        }
    }

    /// Entry point for RabbitMQ consumer callback thread.
    pub async fn handle_delivery(&self, raw_bytes: Vec<u8>, routing_key: String, acker: DeliveryAcker) {
        let start_time = tokio::time::Instant::now();
        tracing::debug!("Received message on routing key: {}", routing_key);

        // 1. Deserialize envelope
        let mut envelope = match TelemetryEnvelope::decode(&*raw_bytes) {
            Ok(env) => env,
            Err(e) => {
                tracing::error!("Failed to deserialize TelemetryEnvelope: {:?}. Rejecting message.", e);
                self.alert_port.emit_critical("deserialization", &format!("Malformed telemetry envelope: {e}")).await;
                acker.nack().await;
                return;
            }
        };

        let mission_code = match &envelope.mission {
            Some(m) if !m.mission_code.is_empty() => m.mission_code.clone(),
            _ => {
                tracing::error!("TelemetryEnvelope missing mission code. Routing to DLQ.");
                self.alert_port.emit_critical("validation", "TelemetryEnvelope missing mission code").await;
                acker.nack().await;
                return;
            }
        };

        // 2. Load mission configuration database
        let db = match self.registry.get_db(&mission_code) {
            Ok(database) => database,
            Err(err) => {
                match err {
                    DomainError::ConfigFileNotFound(_, _) => {
                        tracing::error!("No configuration found for mission '{}': {:?}", mission_code, err);
                        self.alert_port.emit_critical("registry", &format!("Config file not found for mission '{mission_code}': {err}")).await;
                        acker.nack().await;
                    }
                    _ => {
                        tracing::error!("Failed to load config for mission '{}': {:?}", mission_code, err);
                        self.alert_port.emit_critical("registry", &format!("Config error for mission '{mission_code}': {err}")).await;
                        acker.nack().await;
                    }
                }
                return;
            }
        };

        // 3. Perform calculations in topological order
        let mut success_count = 0;
        let mut fail_count = 0;

        for definition in &db.derived_parameters {
            match ComputationEngine::evaluate(definition, &envelope.parameters) {
                Ok(new_param) => {
                    envelope.parameters.push(new_param);
                    success_count += 1;
                }
                Err(err) => {
                    tracing::warn!("Failed to calculate derived parameter '{}': {:?}", definition.name, err);
                    self.alert_port.emit_warning(
                        "computation",
                        &format!("Calculation failed for '{}' on mission '{}': {}", definition.name, mission_code, err),
                    ).await;

                    metrics::counter!(
                        "ecs_formula_failures_total",
                        "mission" => mission_code.clone(),
                        "derived_parameter" => definition.name.clone()
                    ).increment(1);

                    // Append invalid parameter to parameters list
                    envelope.parameters.push(TelemetryParameter {
                        name: definition.name.clone(),
                        raw_value: None,
                        engineering_value: Some(ParameterValue {
                            value: Some(ProtoValue::FloatValue(f64::NAN)),
                        }),
                        validity: ParameterValidity::Invalid as i32,
                    });
                    fail_count += 1;
                }
            }
        }

        // 4. Update processing stage
        envelope.stage = ProcessingStage::EngineeringConverted as i32;

        // 5. Publish enriched envelope back to the telemetry.engineering exchange
        let satellite_id = envelope.satellite.as_ref().map(|s| s.satellite_id).unwrap_or(0);
        let apid = envelope.apid;
        let outbound_routing_key = format!("{mission_code}.sat{satellite_id}.{apid}.engineering");

        match self.publisher.publish(&envelope, &outbound_routing_key).await {
            Ok(_) => {
                tracing::info!(
                    "Successfully processed envelope {}. success={}, failed={}",
                    envelope.envelope_id,
                    success_count,
                    fail_count
                );
                metrics::counter!(
                    "ecs_packets_processed_total",
                    "status" => "success",
                    "mission" => mission_code.clone(),
                    "satellite" => satellite_id.to_string(),
                    "apid" => apid.to_string()
                ).increment(1);
                acker.ack().await;
            }
            Err(pub_err) => {
                tracing::error!("Failed to publish enriched envelope {}: {:?}", envelope.envelope_id, pub_err);
                self.alert_port.emit_critical(
                    "publish",
                    &format!("Failed to publish enriched envelope {}: {pub_err}", envelope.envelope_id),
                ).await;
                metrics::counter!(
                    "ecs_packets_processed_total",
                    "status" => "failed",
                    "mission" => mission_code.clone(),
                    "satellite" => satellite_id.to_string(),
                    "apid" => apid.to_string()
                ).increment(1);
                acker.nack().await;
            }
        }

        metrics::histogram!("ecs_processing_latency_seconds").record(start_time.elapsed().as_secs_f64());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use crate::ports::inbound::DeliveryAcker;
    use crate::ports::outbound::AlertPort;
    use crate::ports::outbound::EngineeringPublisher;
    use crate::proto::{MissionIdentifier, SatelliteIdentifier, TelemetryEnvelope};
    use async_trait::async_trait;
    use tokio::sync::Mutex;

    struct MockPublisher {
        published: Arc<Mutex<Vec<(TelemetryEnvelope, String)>>>,
    }

    #[async_trait]
    impl EngineeringPublisher for MockPublisher {
        async fn publish(&self, envelope: &TelemetryEnvelope, routing_key: &str) -> Result<(), DomainError> {
            let mut list = self.published.lock().await;
            list.push((envelope.clone(), routing_key.to_string()));
            Ok(())
        }
    }

    struct MockAlertPort;

    #[async_trait]
    impl AlertPort for MockAlertPort {
        async fn emit_warning(&self, _context: &str, _message: &str) {}
        async fn emit_critical(&self, _context: &str, _message: &str) {}
    }

    fn setup_test_dir(dir_name: &str) -> PathBuf {
        let path = PathBuf::from(dir_name);
        if path.exists() {
            let _ = fs::remove_dir_all(&path);
        }
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn cleanup_test_dir(path: PathBuf) {
        if path.exists() {
            let _ = fs::remove_dir_all(path);
        }
    }

    #[tokio::test]
    async fn test_orchestrator_handle_delivery_happy_path() {
        let temp_dir = setup_test_dir("./temp_test_orch");
        let mission = "TEST_MISSION";
        let yaml_content = r#"
derived_parameters:
  - name: "/SC/BatteryPower"
    inputs:
      - parameter_name: "/SC/BatteryVoltage"
        alias: "v"
      - parameter_name: "/SC/BatteryCurrent"
        alias: "i"
    expression: "v * i"
"#;
        fs::write(temp_dir.join(format!("{}.yaml", mission)), yaml_content).unwrap();

        let config = Arc::new(AppConfig {
            amqp_url: "amqp://localhost".to_string(),
            source_exchange: "ex1".to_string(),
            source_queue: "q1".to_string(),
            source_routing_key: "rk1".to_string(),
            consumer_tag: "tag1".to_string(),
            prefetch_count: 10,
            destination_exchange: "ex2".to_string(),
            derived_db_dir: temp_dir.to_string_lossy().to_string(),
            metrics_port: 8085,
            health_port: 8086,
            publish_timeout_ms: 1000,
            retry_max_attempts: 3,
        });

        let registry = Arc::new(FormulaRegistry::new(&temp_dir));
        let published_list = Arc::new(Mutex::new(Vec::new()));
        let publisher = Arc::new(MockPublisher {
            published: Arc::clone(&published_list),
        });
        let alerts = Arc::new(MockAlertPort);

        let orchestrator = ConversionOrchestrator::new(config, registry, publisher, alerts);

        // Construct input TelemetryEnvelope
        let mut envelope = TelemetryEnvelope::default();
        envelope.envelope_id = "env-123".to_string();
        envelope.mission = Some(MissionIdentifier {
            mission_id: 1,
            mission_name: "Test Mission".to_string(),
            mission_code: mission.to_string(),
        });
        envelope.satellite = Some(SatelliteIdentifier {
            satellite_id: 1,
            satellite_name: "Test Sat".to_string(),
            norad_id: 12345,
        });
        envelope.apid = 10;
        envelope.parameters = vec![
            TelemetryParameter {
                name: "/SC/BatteryVoltage".to_string(),
                raw_value: None,
                engineering_value: Some(ParameterValue {
                    value: Some(ProtoValue::FloatValue(28.0)),
                }),
                validity: ParameterValidity::Valid as i32,
            },
            TelemetryParameter {
                name: "/SC/BatteryCurrent".to_string(),
                raw_value: None,
                engineering_value: Some(ParameterValue {
                    value: Some(ProtoValue::FloatValue(3.0)),
                }),
                validity: ParameterValidity::Valid as i32,
            },
        ];

        let mut raw_bytes = Vec::new();
        envelope.encode(&mut raw_bytes).unwrap();

        let acker = DeliveryAcker::noop();
        orchestrator.handle_delivery(raw_bytes, "key".to_string(), acker).await;

        let published = published_list.lock().await;
        assert_eq!(published.len(), 1);
        let (out_env, out_routing_key) = &published[0];
        assert_eq!(out_routing_key, "TEST_MISSION.sat1.10.engineering");
        assert_eq!(out_env.stage, ProcessingStage::EngineeringConverted as i32);
        
        // Assert derived parameter is appended
        let power_param = out_env.parameters.iter().find(|p| p.name == "/SC/BatteryPower").unwrap();
        assert_eq!(power_param.validity, ParameterValidity::Valid as i32);
        if let Some(ParameterValue { value: Some(ProtoValue::FloatValue(val)) }) = &power_param.engineering_value {
            assert_eq!(*val, 84.0);
        } else {
            panic!("Expected BatteryPower to be 84.0");
        }

        cleanup_test_dir(temp_dir);
    }
}

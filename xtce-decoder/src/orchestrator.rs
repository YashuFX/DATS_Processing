use std::sync::Arc;
use crate::config::AppConfig;
use crate::domain::errors::XtceError;
use crate::domain::registry::XtceRegistry;
use crate::domain::decommutation::DecommutationEngine;
use crate::domain::calibration::CalibrationEngine;
use crate::ports::inbound::{EnvelopeConsumer, DeliveryAcker};
use crate::ports::outbound::{EngineeringPublisher, AlertPort};
use crate::proto::{TelemetryEnvelope, ProcessingStage, TelemetryParameter};
use prost::Message;

pub struct DecoderOrchestrator {
    config: Arc<AppConfig>,
    registry: Arc<XtceRegistry>,
    consumer: Arc<dyn EnvelopeConsumer>,
    publisher: Arc<dyn EngineeringPublisher>,
    alert_port: Arc<dyn AlertPort>,
}

impl DecoderOrchestrator {
    pub fn new(
        config: Arc<AppConfig>,
        registry: Arc<XtceRegistry>,
        consumer: Arc<dyn EnvelopeConsumer>,
        publisher: Arc<dyn EngineeringPublisher>,
        alert_port: Arc<dyn AlertPort>,
    ) -> Self {
        Self {
            config,
            registry,
            consumer,
            publisher,
            alert_port,
        }
    }

    /// Entry point to start consuming and decommutating messages with auto-reconnection.
    pub async fn start(&self) -> Result<(), XtceError> {
        let orchestrator = Arc::new(self.clone_refs());
        
        let handler = Arc::new(move |raw_bytes: Vec<u8>, routing_key: String, acker: DeliveryAcker| {
            let orch = orchestrator.clone();
            Box::pin(async move {
                if let Err(e) = orch.process_message(raw_bytes, routing_key, acker).await {
                    tracing::error!("Failed to process message: {:?}", e);
                }
            }) as futures::future::BoxFuture<'static, ()>
        });

        tracing::info!("Starting XTCE Decoder Orchestrator consumer loop...");
        loop {
            match self.consumer.start(handler.clone()).await {
                Ok(_) => {
                    tracing::warn!("AMQP consumer connection closed cleanly. Reconnecting in 5 seconds...");
                }
                Err(e) => {
                    tracing::error!("AMQP consumer error: {:?}. Reconnecting in 5 seconds...", e);
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }

    fn clone_refs(&self) -> Self {
        Self {
            config: self.config.clone(),
            registry: self.registry.clone(),
            consumer: self.consumer.clone(),
            publisher: self.publisher.clone(),
            alert_port: self.alert_port.clone(),
        }
    }

    async fn process_message(
        &self,
        raw_bytes: Vec<u8>,
        _original_routing_key: String,
        acker: DeliveryAcker,
    ) -> Result<(), XtceError> {
        // 1. Deserialize envelope
        let mut envelope = match TelemetryEnvelope::decode(raw_bytes.as_slice()) {
            Ok(env) => env,
            Err(e) => {
                let err_msg = format!("Failed to decode TelemetryEnvelope: {e}");
                self.alert_port.emit_warning("Orchestrator::Decode", &err_msg).await;
                acker.ack().await;
                return Err(XtceError::DecommutationFailed(err_msg));
            }
        };

        let envelope_id = envelope.envelope_id.clone();

        // 2. Validate metadata fields
        let mission_code = match envelope.mission.as_ref().map(|m| m.mission_code.clone()) {
            Some(code) => code,
            None => {
                let err_msg = format!("Envelope ID={envelope_id} lacks a mission identifier");
                self.alert_port.emit_warning("Orchestrator::Validation", &err_msg).await;
                acker.ack().await;
                return Ok(());
            }
        };

        let apid = envelope.apid;

        // 3. Load/Get XTCE database
        let db = match self.registry.get_db(&mission_code) {
            Ok(database) => database,
            Err(e) => {
                let err_msg = format!("Failed to fetch XTCE database for mission '{mission_code}': {:?}", e);
                self.alert_port.emit_warning("Orchestrator::Database", &err_msg).await;
                // Requeue = false nack
                acker.nack().await;
                return Err(e);
            }
        };

        // 4. Retrieve matching SequenceContainer layout
        let container = match db.containers.get(&apid) {
            Some(c) => c,
            None => {
                let warn_msg = format!("No XTCE sequence container defined for APID {apid} in mission '{mission_code}'");
                tracing::warn!("{}", warn_msg);
                
                envelope.annotations.insert("xtce.warning".to_string(), warn_msg);
                envelope.stage = ProcessingStage::Engineering as i32;

                let satellite_id = envelope.satellite.as_ref().map(|s| s.satellite_id).unwrap_or(0);
                let outbound_routing_key = format!("{mission_code}.{satellite_id}.{apid}.decommutated");
                if let Err(pub_err) = self.publisher.publish(&envelope, &outbound_routing_key).await {
                    self.alert_port.emit_critical("Orchestrator::Publish", &format!("Failed to publish packet: {:?}", pub_err)).await;
                    acker.nack().await;
                    return Err(pub_err);
                }
                acker.ack().await;
                return Ok(());
            }
        };

        // 5. Extract packet data payload
        let raw_payload = match &envelope.raw_packet {
            Some(p) => &p.data,
            None => {
                let err_msg = format!("Envelope ID={envelope_id} has no raw packet payload");
                self.alert_port.emit_warning("Orchestrator::Decommutation", &err_msg).await;
                acker.ack().await;
                return Ok(());
            }
        };

        // 6. Decommute parameters
        let decom_params = match DecommutationEngine::decommute(raw_payload, container, &db) {
            Ok(params) => params,
            Err(e) => {
                let err_msg = format!("Decommutation failed for envelope ID={envelope_id}: {:?}", e);
                self.alert_port.emit_warning("Orchestrator::Decommutation", &err_msg).await;
                acker.ack().await;
                return Ok(());
            }
        };

        // 7. Calibrate decommutated parameter stream
        let mut telemetry_parameters = Vec::new();
        for decom_param in decom_params {
            let param_def = db.parameters.get(&decom_param.name).unwrap();
            match CalibrationEngine::calibrate(&decom_param, param_def) {
                Ok(telemetry_param) => {
                    telemetry_parameters.push(telemetry_param);
                }
                Err(e) => {
                    tracing::warn!("Calibration failed for parameter {}: {:?}", decom_param.name, e);
                    telemetry_parameters.push(TelemetryParameter {
                        name: decom_param.name.clone(),
                        raw_value: Some(decom_param.raw_value.clone()),
                        engineering_value: Some(decom_param.raw_value.clone()),
                        validity: crate::proto::ParameterValidity::Invalid as i32,
                    });
                }
            }
        }

        // 8. Enrich envelope
        envelope.parameters = telemetry_parameters;
        envelope.stage = ProcessingStage::Engineering as i32;

        // 9. Publish downstream
        let satellite_id = envelope.satellite.as_ref().map(|s| s.satellite_id).unwrap_or(0);
        let outbound_routing_key = format!("{mission_code}.{satellite_id}.{apid}.decommutated");
        
        if let Err(pub_err) = self.publisher.publish(&envelope, &outbound_routing_key).await {
            self.alert_port.emit_critical("Orchestrator::Publish", &format!("Failed to publish decommutated envelope: {:?}", pub_err)).await;
            acker.nack().await;
            return Err(pub_err);
        }

        tracing::info!(
            "[XTCE ✓] Decoded parameters for EnvID={} | Mission={} | APID={} | Params={:?}",
            envelope_id,
            mission_code,
            apid,
            envelope.parameters
        );

        // 10. Acknowledge receipt
        acker.ack().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::inbound::HandlerFn;
    use crate::proto::{RawTelemetryPacket, TelemetryEnvelope, parameter_value::Value, ParameterValidity, ProcessingStage};
    use crate::proto::must::common::v1::{MissionIdentifier, SatelliteIdentifier};
    use std::sync::Mutex;

    struct FakePublisher {
        published: Arc<Mutex<Vec<(TelemetryEnvelope, String)>>>,
    }

    #[async_trait::async_trait]
    impl EngineeringPublisher for FakePublisher {
        async fn publish(&self, envelope: &TelemetryEnvelope, routing_key: &str) -> Result<(), XtceError> {
            let mut list = self.published.lock().unwrap();
            list.push((envelope.clone(), routing_key.to_string()));
            Ok(())
        }
    }

    struct FakeAlertPort;

    #[async_trait::async_trait]
    impl AlertPort for FakeAlertPort {
        async fn emit_warning(&self, _context: &str, _message: &str) {}
        async fn emit_critical(&self, _context: &str, _message: &str) {}
    }

    struct FakeConsumer;

    #[async_trait::async_trait]
    impl EnvelopeConsumer for FakeConsumer {
        async fn start(&self, _handler: HandlerFn) -> Result<(), XtceError> {
            Ok(())
        }
    }

    const TEST_XTCE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<SpaceSystem xmlns="http://www.omg.org/space/xtce" name="TestMission">
  <TelemetryMetadata>
    <ParameterSet>
      <Parameter name="Volt" parameterTypeRef="VoltType"/>
      <Parameter name="Temp" parameterTypeRef="TempType"/>
      <Parameter name="Status" parameterTypeRef="StatusType"/>

      <ParameterTypeSet>
        <IntegerParameterType name="VoltType" signed="false">
          <IntegerDataEncoding sizeInBits="12"/>
          <DefaultCalibrator>
            <PolynomialCalibrator>
              <Term coefficient="0.02" exponent="1"/>
            </PolynomialCalibrator>
          </DefaultCalibrator>
        </IntegerParameterType>

        <IntegerParameterType name="TempType" signed="true">
          <IntegerDataEncoding sizeInBits="8"/>
        </IntegerParameterType>

        <IntegerParameterType name="StatusType" signed="false">
          <IntegerDataEncoding sizeInBits="4"/>
          <DefaultCalibrator>
            <StateCalibrator>
              <State value="0" label="OFF"/>
              <State value="1" label="ON"/>
              <State value="10" label="TRICKLE"/>
            </StateCalibrator>
          </DefaultCalibrator>
        </IntegerParameterType>
      </ParameterTypeSet>
    </ParameterSet>

    <ContainerSet>
      <SequenceContainer name="MainContainer" apid="42">
        <EntryList>
          <ParameterRefEntry parameterRef="Volt"/>
          <ParameterRefEntry parameterRef="Temp"/>
          <ParameterRefEntry parameterRef="Status"/>
        </EntryList>
      </SequenceContainer>
    </ContainerSet>
  </TelemetryMetadata>
</SpaceSystem>
"#;

    #[tokio::test]
    async fn test_orchestrator_processing_flow() {
        // Setup config and registry
        let config = Arc::new(AppConfig::from_env().unwrap_or_else(|_| {
            std::env::set_var("AMQP_URL", "amqp://test");
            AppConfig::from_env().unwrap()
        }));
        
        let registry = Arc::new(XtceRegistry::new("".to_string()));
        let db = XtceRegistry::parse_xtce("test", TEST_XTCE).unwrap();
        {
            let mut cache = registry.cache.write().unwrap();
            cache.insert("test".to_string(), Arc::new(db));
        }

        let consumer = Arc::new(FakeConsumer);
        let published_list = Arc::new(Mutex::new(Vec::new()));
        let publisher = Arc::new(FakePublisher { published: published_list.clone() });
        let alert_port = Arc::new(FakeAlertPort);

        let orchestrator = DecoderOrchestrator::new(
            config,
            registry,
            consumer,
            publisher,
            alert_port,
        );

        // Construct a telemetry envelope with a raw packet payload
        // Volt = 12 bits, Temp = 8 bits, Status = 4 bits
        // Payload: [0b1010_1100, 0b0011_1111, 0b1111_0001]
        // Volt (bits 0-11): 0b1010_1100_0011 = 2755
        // Temp (bits 12-19): 0b1111_1111 = -1 signed 8-bit
        // Status (bits 20-23): 0b0001 = 1 unsigned 4-bit (ON)
        let mut envelope = TelemetryEnvelope::default();
        envelope.envelope_id = "test-uuid".to_string();
        envelope.mission = Some(MissionIdentifier {
            mission_id: 200,
            mission_code: "test".to_string(),
            mission_name: "Test Mission".to_string(),
        });
        envelope.satellite = Some(SatelliteIdentifier {
            satellite_id: 101,
            satellite_name: "Test Sat".to_string(),
            norad_id: 0,
        });
        envelope.apid = 42;
        envelope.raw_packet = Some(RawTelemetryPacket {
            data: vec![0b1010_1100, 0b0011_1111, 0b1111_0001],
            data_length: 3,
            receive_time: None,
            file_offset: 0,
        });

        let mut raw_envelope_bytes = Vec::new();
        envelope.encode(&mut raw_envelope_bytes).unwrap();

        // Process message
        orchestrator
            .process_message(raw_envelope_bytes, "test.routing".to_string(), DeliveryAcker::noop())
            .await
            .unwrap();

        // Assert output
        let published = published_list.lock().unwrap();
        assert_eq!(published.len(), 1);
        
        let (output_env, rkey) = &published[0];
        assert_eq!(rkey, "test.101.42.decommutated");
        assert_eq!(output_env.stage, ProcessingStage::Engineering as i32);
        assert_eq!(output_env.parameters.len(), 3);

        // Param 1: Volt (polynomial calibration)
        let volt = &output_env.parameters[0];
        assert_eq!(volt.name, "Volt");
        assert_eq!(volt.raw_value.as_ref().unwrap().value, Some(Value::IntValue(2755)));
        assert_eq!(volt.engineering_value.as_ref().unwrap().value, Some(Value::FloatValue(2755.0 * 0.02)));
        assert_eq!(volt.validity, ParameterValidity::Valid as i32);

        // Param 2: Temp (signed, no calibration)
        let temp = &output_env.parameters[1];
        assert_eq!(temp.name, "Temp");
        assert_eq!(temp.raw_value.as_ref().unwrap().value, Some(Value::IntValue(-1)));
        assert_eq!(temp.engineering_value.as_ref().unwrap().value, Some(Value::IntValue(-1)));
        assert_eq!(temp.validity, ParameterValidity::Valid as i32);

        // Param 3: Status (state calibration)
        let status = &output_env.parameters[2];
        assert_eq!(status.name, "Status");
        assert_eq!(status.raw_value.as_ref().unwrap().value, Some(Value::IntValue(1)));
        assert_eq!(status.engineering_value.as_ref().unwrap().value, Some(Value::StringValue("ON".to_string())));
        assert_eq!(status.validity, ParameterValidity::Valid as i32);
    }

    #[tokio::test]
    async fn test_load_and_leak_benchmark() {
        let config = Arc::new(AppConfig::from_env().unwrap_or_else(|_| {
            std::env::set_var("AMQP_URL", "amqp://test");
            AppConfig::from_env().unwrap()
        }));
        
        let registry = Arc::new(XtceRegistry::new("".to_string()));
        let db = XtceRegistry::parse_xtce("test", TEST_XTCE).unwrap();
        {
            let mut cache = registry.cache.write().unwrap();
            cache.insert("test".to_string(), Arc::new(db));
        }

        let consumer = Arc::new(FakeConsumer);
        struct NoopPublisher;
        #[async_trait::async_trait]
        impl EngineeringPublisher for NoopPublisher {
            async fn publish(&self, _envelope: &TelemetryEnvelope, _routing_key: &str) -> Result<(), XtceError> {
                Ok(())
            }
        }
        let publisher = Arc::new(NoopPublisher);
        let alert_port = Arc::new(FakeAlertPort);

        let orchestrator = DecoderOrchestrator::new(
            config,
            registry,
            consumer,
            publisher,
            alert_port,
        );

        let mut envelope = TelemetryEnvelope::default();
        envelope.envelope_id = "test-uuid".to_string();
        envelope.mission = Some(MissionIdentifier {
            mission_id: 200,
            mission_code: "test".to_string(),
            mission_name: "Test Mission".to_string(),
        });
        envelope.satellite = Some(SatelliteIdentifier {
            satellite_id: 101,
            satellite_name: "Test Sat".to_string(),
            norad_id: 0,
        });
        envelope.apid = 42;
        envelope.raw_packet = Some(RawTelemetryPacket {
            data: vec![0b1010_1100, 0b0011_1111, 0b1111_0001],
            data_length: 3,
            receive_time: None,
            file_offset: 0,
        });

        let mut raw_envelope_bytes = Vec::new();
        envelope.encode(&mut raw_envelope_bytes).unwrap();

        // Warm up
        for _ in 0..10 {
            orchestrator
                .process_message(raw_envelope_bytes.clone(), "test.routing".to_string(), DeliveryAcker::noop())
                .await
                .unwrap();
        }

        let start = std::time::Instant::now();
        let iterations = 100_000;
        
        for _ in 0..iterations {
            orchestrator
                .process_message(raw_envelope_bytes.clone(), "test.routing".to_string(), DeliveryAcker::noop())
                .await
                .unwrap();
        }

        let elapsed = start.elapsed();
        let throughput = (iterations as f64) / elapsed.as_secs_f64();
        let latency_us = (elapsed.as_micros() as f64) / (iterations as f64);
        
        println!(
            "\n[BENCHMARK] Processed {iterations} packets in {elapsed:?}. Throughput: {throughput:.2} packets/sec. Avg Latency: {latency_us:.3} µs/packet\n"
        );
        
        assert!(throughput > 10000.0);
    }
}

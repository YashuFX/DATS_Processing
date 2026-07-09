pub mod must {
    pub mod telemetry {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/must.telemetry.v1.rs"));
        }
    }
    pub mod common {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/must.common.v1.rs"));
        }
    }
}

pub use must::telemetry::v1::ProcessingStage;
pub use must::telemetry::v1::QualityIndicator;
pub use must::telemetry::v1::RawTelemetryPacket;
pub use must::telemetry::v1::TelemetryEnvelope;

pub use must::common::v1::MissionIdentifier;
pub use must::common::v1::SatelliteIdentifier;
pub use must::common::v1::MustTimestamp;
pub use must::common::v1::SourceIdentifier;
pub use must::common::v1::SourceType;

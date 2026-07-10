// ── Protobuf Generated Type Re-exports ───────────────────────────────────────
//
// This is the ONE place in the crate that touches `OUT_DIR`.
// All other modules import via `use crate::proto::*`.

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

// Convenience re-exports
pub use must::telemetry::v1::TelemetryEnvelope;
pub use must::telemetry::v1::TelemetryParameter;
pub use must::telemetry::v1::ParameterValue;
pub use must::telemetry::v1::parameter_value;
pub use must::telemetry::v1::ParameterValidity;
pub use must::telemetry::v1::ProcessingStage;
pub use must::telemetry::v1::RawTelemetryPacket;
pub use must::telemetry::v1::CcsdsPacketHeader;
pub use must::telemetry::v1::CcsdsSecondaryHeader as ProtoCcsdsSecondaryHeader;

pub use must::common::v1::MissionIdentifier;
pub use must::common::v1::SatelliteIdentifier;
pub use must::common::v1::MustTimestamp;
pub use must::common::v1::TimestampSource;

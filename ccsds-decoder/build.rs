fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure().compile(
        &[
            "../shared/proto/must/common/v1/identifiers.proto",
            "../shared/proto/must/common/v1/timestamps.proto",
            "../shared/proto/must/telemetry/v1/packet.proto",
            "../shared/proto/must/telemetry/v1/ccsds.proto",
            "../shared/proto/must/telemetry/v1/envelope.proto",
        ],
        &["../shared/proto"],
    )?;
    Ok(())
}

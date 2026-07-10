use std::env;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Dynamically locate the local protoc binary in the workspace bin directory
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    let workspace_dir = Path::new(&manifest_dir).parent().ok_or("Cannot find parent workspace directory")?;
    let protoc_path = workspace_dir.join("bin/bin/protoc");
    
    if protoc_path.exists() {
        env::set_var("PROTOC", protoc_path.to_str().unwrap());
        println!("cargo:rustc-env=PROTOC={}", protoc_path.to_str().unwrap());
    } else {
        println!("cargo:warning=Local protoc not found at: {:?}", protoc_path);
    }

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

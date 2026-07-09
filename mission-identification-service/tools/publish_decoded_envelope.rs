use lapin::{
    options::BasicPublishOptions, types::FieldTable, BasicProperties, Connection,
    ConnectionProperties,
};
use prost::Message;
use std::env;

pub mod proto {
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
    pub use must::telemetry::v1::TelemetryEnvelope;
    pub use must::common::v1::SourceIdentifier;
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let amqp_url = env::var("AMQP_URL")
        .unwrap_or_else(|_| "amqp://guest:guest@localhost:5672/%2f".to_string());

    println!("Connecting to RabbitMQ at {amqp_url}...");
    let conn = Connection::connect(&amqp_url, ConnectionProperties::default()).await?;
    let channel = conn.create_channel().await?;

    let exchange = "telemetry.decoded";
    let routing_key = "cy3.sat101.42.decoded";

    channel
        .exchange_declare(
            exchange,
            lapin::ExchangeKind::Topic,
            lapin::options::ExchangeDeclareOptions {
                durable: true,
                ..lapin::options::ExchangeDeclareOptions::default()
            },
            FieldTable::default(),
        )
        .await?;

    let envelope = proto::TelemetryEnvelope {
        envelope_id: "smoke-test-mis-id-888".to_string(),
        sequence_number: 98765,
        apid: 42,
        vcid: 0,
        stage: 2, // PROCESSING_STAGE_CCSDS_DECODED
        source: Some(proto::SourceIdentifier {
            source_id: "rss-replay".to_string(),
            source_type: 1,
            source_name: "Replay".to_string(),
        }),
        ..Default::default()
    };

    let mut payload = Vec::new();
    envelope.encode(&mut payload)?;

    println!("Publishing test envelope (ID=smoke-test-mis-id-888) to exchange '{exchange}' with routing key '{routing_key}'...");
    channel
        .basic_publish(
            exchange,
            routing_key,
            BasicPublishOptions::default(),
            &payload,
            BasicProperties::default(),
        )
        .await?
        .await?;

    println!("Successfully published test envelope!");
    Ok(())
}

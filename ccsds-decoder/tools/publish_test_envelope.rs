// ── Integration Smoke Test Helper Binary ─────────────────────────────────────
//
// Responsibility: Connect to RabbitMQ, declare the raw exchange,
// and publish a single synthetic TelemetryEnvelope to trigger the decoder.

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
    pub use must::telemetry::v1::RawTelemetryPacket;
    pub use must::telemetry::v1::TelemetryEnvelope;
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let amqp_url = env::var("AMQP_URL")
        .unwrap_or_else(|_| "amqp://guest:guest@localhost:5672/%2f".to_string());

    println!("Publisher connecting to RabbitMQ at {amqp_url}...");
    let conn = Connection::connect(&amqp_url, ConnectionProperties::default()).await?;
    let channel = conn.create_channel().await?;

    let exchange = "telemetry.raw";
    let routing_key = "cy3.sat101.42.raw";

    // Declare the exchange just in case the consumer hasn't run yet
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

    // Create the test telemetry packet
    // Word 0: version=0, type=TM (0), sec_hdr=0, APID=42 (0x002A)
    // Word 1: seq_flags=standalone (3), seq_count=1200 (0x04B0) -> 0b11_00_0100_1011_0000 = 0xC4B0
    // Word 2: data_length = 21 (0x0015)
    let mut raw_data = vec![
        0x00, 0x2A, // Word 0
        0xC4, 0xB0, // Word 1
        0x00, 0x15, // Word 2
    ];
    // Pad 22 bytes of user data
    raw_data.extend(vec![0xAA; 22]);

    let envelope = proto::TelemetryEnvelope {
        envelope_id: "smoke-test-env-id-999".to_string(),
        sequence_number: 12345,
        raw_packet: Some(proto::RawTelemetryPacket {
            data: raw_data,
            data_length: 28,
            ..Default::default()
        }),
        ..Default::default()
    };

    let mut payload = Vec::new();
    envelope.encode(&mut payload)?;

    println!("Publishing test envelope (ID=smoke-test-env-id-999) to exchange '{exchange}' with routing key '{routing_key}'...");
    channel
        .basic_publish(
            exchange,
            routing_key,
            BasicPublishOptions::default(),
            &payload,
            BasicProperties::default(),
        )
        .await?
        .await?; // await publisher confirm/completion

    println!("Successfully published test envelope!");
    Ok(())
}

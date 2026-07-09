use futures::StreamExt;
use lapin::{
    options::{BasicConsumeOptions, QueueBindOptions, QueueDeclareOptions},
    types::FieldTable,
    Connection, ConnectionProperties,
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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let amqp_url = env::var("AMQP_URL")
        .unwrap_or_else(|_| "amqp://guest:guest@localhost:5672/%2f".to_string());

    let dest_exchange =
        env::var("DESTINATION_EXCHANGE").unwrap_or_else(|_| "telemetry.identified".to_string());

    println!("Connecting to RabbitMQ at {amqp_url}...");
    let conn = Connection::connect(&amqp_url, ConnectionProperties::default()).await?;
    let channel = conn.create_channel().await?;

    channel
        .exchange_declare(
            &dest_exchange,
            lapin::ExchangeKind::Topic,
            lapin::options::ExchangeDeclareOptions {
                durable: true,
                ..Default::default()
            },
            lapin::types::FieldTable::default(),
        )
        .await?;

    let queue = channel
        .queue_declare(
            "",
            QueueDeclareOptions {
                exclusive: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;

    let queue_name = queue.name().as_str();

    channel
        .queue_bind(
            queue_name,
            &dest_exchange,
            "#",
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let mut consumer = channel
        .basic_consume(
            queue_name,
            "identified-consumer-test-tag",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    println!("Awaiting identified envelope on exchange '{dest_exchange}'...");

    if let Some(delivery_result) = consumer.next().await {
        let delivery = delivery_result?;
        println!("Received message! Routing key: {}", delivery.routing_key);

        let envelope = proto::TelemetryEnvelope::decode(&delivery.data[..])?;

        println!("=== Identified Envelope Analysis ===");
        println!("Envelope ID:       {}", envelope.envelope_id);
        println!("Stage:             {}", envelope.stage);
        
        if let Some(mission) = &envelope.mission {
            println!("Mission ID:        {}", mission.mission_id);
            println!("Mission Name:      {}", mission.mission_name);
            println!("Mission Code:      {}", mission.mission_code);
            println!("[ASSERT_MISSION_CODE={}]", mission.mission_code);
        } else {
            println!("  [WARN] mission identifier is None!");
        }

        if let Some(sat) = &envelope.satellite {
            println!("Satellite ID:      {}", sat.satellite_id);
            println!("Satellite Name:    {}", sat.satellite_name);
            println!("NORAD ID:          {}", sat.norad_id);
            println!("[ASSERT_SAT_ID={}]", sat.satellite_id);
        } else {
            println!("  [WARN] satellite identifier is None!");
        }

        println!("[ASSERT_STAGE={}]", envelope.stage);
        println!("[ASSERT_ROUTING_KEY={}]", delivery.routing_key);

        channel
            .basic_ack(
                delivery.delivery_tag,
                lapin::options::BasicAckOptions::default(),
            )
            .await?;
    }

    Ok(())
}

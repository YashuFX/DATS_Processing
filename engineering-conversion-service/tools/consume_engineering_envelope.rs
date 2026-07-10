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
    pub use must::telemetry::v1::{TelemetryEnvelope, parameter_value::Value};
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let amqp_url = env::var("AMQP_URL")
        .unwrap_or_else(|_| "amqp://guest:guest@localhost:5672/%2f".to_string());

    let dest_exchange =
        env::var("DESTINATION_EXCHANGE").unwrap_or_else(|_| "telemetry.engineering".to_string());

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

    // Bind to the specific engineering converted routing key pattern
    channel
        .queue_bind(
            queue_name,
            &dest_exchange,
            "#.engineering",
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let mut consumer = channel
        .basic_consume(
            queue_name,
            "ecs-consumer-test-tag",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    println!("Awaiting engineering envelope on exchange '{dest_exchange}'...");

    while let Some(delivery_result) = consumer.next().await {
        let delivery = delivery_result?;
        let envelope = proto::TelemetryEnvelope::decode(&delivery.data[..])?;

        // Acknowledge the message immediately
        channel
            .basic_ack(
                delivery.delivery_tag,
                lapin::options::BasicAckOptions::default(),
            )
            .await?;

        // Check if this is the packet we published
        if envelope.envelope_id == "smoke-test-ecs-999" {
            println!("Received smoke test message! Routing key: {}", delivery.routing_key);
            println!("=== Engineering Envelope Analysis ===");
            println!("Envelope ID:       {}", envelope.envelope_id);
            println!("Stage:             {}", envelope.stage);
            println!("[ASSERT_STAGE={}]", envelope.stage);
            println!("[ASSERT_ROUTING_KEY={}]", delivery.routing_key);

            for param in &envelope.parameters {
                if param.name == "/SC/BatteryPower" {
                    if let Some(val) = &param.engineering_value {
                        if let Some(proto::Value::FloatValue(fval)) = &val.value {
                            println!("Calculated BatteryPower: {}", fval);
                            println!("[ASSERT_BATTERY_POWER={}]", fval);
                        }
                    }
                }
            }
            break;
        } else {
            println!("Skipping unrelated background envelope: {}", envelope.envelope_id);
        }
    }

    Ok(())
}

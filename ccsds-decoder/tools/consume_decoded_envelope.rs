// ── Integration Test Helper: Consume Decoded Envelope ───────────────────────
//
// This helper binary connects to RabbitMQ, binds a temporary queue to the
// destination exchange (e.g. telemetry.decoded), consumes one decoded envelope,
// asserts all its structural mutations, and exits.

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
        env::var("DESTINATION_EXCHANGE").unwrap_or_else(|_| "telemetry.decoded".to_string());

    println!(
        "Decoder test helper connecting to RabbitMQ at {}...",
        amqp_url
    );
    let conn = Connection::connect(&amqp_url, ConnectionProperties::default()).await?;
    let channel = conn.create_channel().await?;

    // Declare the destination exchange
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

    // Declare a temporary exclusive queue
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

    // Bind to the destination exchange
    println!(
        "Binding temporary queue '{}' to exchange '{}' with routing key '#'",
        queue_name, dest_exchange
    );
    channel
        .queue_bind(
            queue_name,
            &dest_exchange,
            "#",
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;

    // Consume one message
    let mut consumer = channel
        .basic_consume(
            queue_name,
            "decoded-consumer-test-tag",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    println!(
        "Awaiting decoded envelope on exchange '{}'...",
        dest_exchange
    );

    if let Some(delivery_result) = consumer.next().await {
        let delivery = delivery_result?;
        println!("Received message! Routing key: {}", delivery.routing_key);

        let envelope = proto::TelemetryEnvelope::decode(&delivery.data[..])?;

        println!("=== Decoded Envelope Analysis ===");
        println!("Envelope ID:       {}", envelope.envelope_id);
        println!("Sequence Number:   {}", envelope.sequence_number);
        println!("Stage:             {}", envelope.stage);
        println!("APID:              {}", envelope.apid);

        if let Some(hdr) = &envelope.ccsds_header {
            println!("--- CCSDS Primary Header ---");
            println!("  Version:         {}", hdr.version_number);
            println!("  Packet Type:     {}", hdr.packet_type);
            println!("  Sec Hdr Flag:    {}", hdr.secondary_header_flag);
            println!("  APID:            {}", hdr.apid);
            println!("  Sequence Flags:  {}", hdr.sequence_flags);
            println!("  Sequence Count:  {}", hdr.sequence_count);
            println!("  Data Length:     {}", hdr.data_length);
        } else {
            println!("  [WARN] ccsds_header is None!");
        }

        if let Some(sec) = &envelope.ccsds_secondary {
            println!("--- CCSDS Secondary Header ---");
            println!("  Coarse Time:     {}", sec.coarse_time);
            println!("  Fine Time:       {}", sec.fine_time);
            println!("  Format:          {}", sec.format);
        }

        if let Some(quality) = &envelope.quality {
            println!("--- Quality Indicator ---");
            println!("  Is Valid:        {}", quality.is_valid);
            println!("  CRC OK:          {}", quality.crc_ok);
            println!("  Seq Continuous:  {}", quality.sequence_continuous);
        } else {
            println!("  [WARN] quality indicator is None!");
        }

        if let Some(pub_ts) = &envelope.publish_timestamp {
            println!(
                "Publish Timestamp: {} ns (source={})",
                pub_ts.nanos_since_epoch, pub_ts.source
            );
        } else {
            println!("  [WARN] publish_timestamp is None!");
        }

        // Output specific assertions for the smoke test runner to scrape
        println!("[ASSERT_STAGE={}]", envelope.stage);
        println!("[ASSERT_APID={}]", envelope.apid);
        if let Some(hdr) = &envelope.ccsds_header {
            println!("[ASSERT_HDR_APID={}]", hdr.apid);
            println!("[ASSERT_HDR_SEQ={}]", hdr.sequence_count);
            println!("[ASSERT_HDR_TYPE={}]", hdr.packet_type);
        }
        if let Some(quality) = &envelope.quality {
            println!("[ASSERT_CRC_OK={}]", quality.crc_ok);
            println!("[ASSERT_SEQ_CONT={}]", quality.sequence_continuous);
        }
        if let Some(pub_ts) = &envelope.publish_timestamp {
            println!("[ASSERT_PUB_TS_SOURCE={}]", pub_ts.source);
        }
        println!("[ASSERT_ROUTING_KEY={}]", delivery.routing_key);

        // Ack message and exit
        channel
            .basic_ack(
                delivery.delivery_tag,
                lapin::options::BasicAckOptions::default(),
            )
            .await?;
    }

    Ok(())
}

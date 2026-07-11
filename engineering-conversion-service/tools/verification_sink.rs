use futures::StreamExt;
use lapin::{
    options::{BasicConsumeOptions, QueueBindOptions, QueueDeclareOptions},
    types::FieldTable,
    Connection, ConnectionProperties,
};
use prost::Message;
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{Write, BufWriter};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

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
    pub use must::telemetry::v1::{TelemetryEnvelope, TelemetryParameter, ParameterValue, parameter_value::Value, ParameterValidity};
    pub use must::common::v1::MustTimestamp;
}

struct RunStats {
    total_received: u64,
    by_apid: HashMap<u32, u64>,
    sequence_gaps: u64,
    invalid_crc: u64,
    last_seq_count: HashMap<u32, u32>,
    
    // Latency aggregation (in ns)
    latency_min: u64,
    latency_max: u64,
    latency_sum: u64,
    latencies: Vec<u64>, // Keep a sample or all for percentile calculation (1M fits in memory easily: 8MB)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let amqp_url = env::var("AMQP_URL")
        .unwrap_or_else(|_| "amqp://guest:guest@localhost:5672/%2f".to_string());

    let dest_exchange =
        env::var("DESTINATION_EXCHANGE").unwrap_or_else(|_| "telemetry.engineering".to_string());

    let csv_path = env::var("CSV_OUTPUT_PATH")
        .unwrap_or_else(|_| "/tmp/verification_packets.csv".to_string());

    let json_path = env::var("JSON_OUTPUT_PATH")
        .unwrap_or_else(|_| "/tmp/verification_summary.json".to_string());

    println!("Verification Sink: Connecting to RabbitMQ at {amqp_url}...");
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

    // Bind to all engineering packets (.engineering suffix)
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
            "verification-sink-tag",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    println!("Verification Sink: Writing packets to {csv_path}...");
    let csv_file = File::create(&csv_path)?;
    let mut csv_writer = BufWriter::new(csv_file);
    
    // Header
    writeln!(
        csv_writer,
        "envelope_id,sequence_number,apid,original_timestamp_ns,latency_ns,volt,temp,status,stage"
    )?;
    csv_writer.flush()?;

    let stats = Arc::new(Mutex::new(RunStats {
        total_received: 0,
        by_apid: HashMap::new(),
        sequence_gaps: 0,
        invalid_crc: 0,
        last_seq_count: HashMap::new(),
        latency_min: u64::MAX,
        latency_max: 0,
        latency_sum: 0,
        latencies: Vec::with_capacity(1_000_000),
    }));

    // Setup graceful shutdown handler to dump stats
    let stats_clone = stats.clone();
    let json_path_clone = json_path.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        println!("\nVerification Sink: Interrupted! Writing summary statistics...");
        let s = stats_clone.lock().await;
        if let Err(e) = write_summary(&s, &json_path_clone) {
            eprintln!("Failed to write summary: {}", e);
        } else {
            println!("Summary written to {}", json_path_clone);
        }
        std::process::exit(0);
    });

    println!("Verification Sink: Awaiting telemetry envelopes on exchange '{dest_exchange}'...");

    while let Some(delivery_result) = consumer.next().await {
        let delivery = delivery_result?;
        let envelope = proto::TelemetryEnvelope::decode(&delivery.data[..])?;

        // Acknowledge immediate
        channel
            .basic_ack(
                delivery.delivery_tag,
                lapin::options::BasicAckOptions::default(),
            )
            .await?;

        // Process timestamps and latency
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let orig_ts = envelope.original_timestamp.as_ref().map(|t| t.nanos_since_epoch).unwrap_or(0);
        let latency = now_ns.saturating_sub(orig_ts);

        // Parameters extraction
        let mut volt_val = "".to_string();
        let mut temp_val = "".to_string();
        let mut status_val = "".to_string();

        for param in &envelope.parameters {
            let val_str = match &param.engineering_value {
                Some(v) => match &v.value {
                    Some(proto::Value::FloatValue(f)) => format!("{:.4}", f),
                    Some(proto::Value::IntValue(i)) => format!("{}", i),
                    Some(proto::Value::StringValue(s)) => s.clone(),
                    Some(proto::Value::BoolValue(b)) => format!("{}", b),
                    Some(_) => "Unsupported".to_string(),
                    None => "NaN".to_string(),
                },
                None => "".to_string(),
            };

            if param.name.ends_with("Volt") || param.name.contains("BatteryVoltage") {
                volt_val = val_str;
            } else if param.name.ends_with("Temp") || param.name.contains("BatteryCurrent") {
                temp_val = val_str;
            } else if param.name.ends_with("Status") {
                status_val = val_str;
            }
        }

        // Write row
        writeln!(
            csv_writer,
            "{},{},{},{},{},{},{},{},{}",
            envelope.envelope_id,
            envelope.sequence_number,
            envelope.apid,
            orig_ts,
            latency,
            volt_val,
            temp_val,
            status_val,
            envelope.stage
        )?;
        
        // Update stats
        {
            let mut s = stats.lock().await;
            s.total_received += 1;
            *s.by_apid.entry(envelope.apid).or_insert(0) += 1;
            
            // Check CRC quality flag
            if let Some(q) = &envelope.quality {
                if !q.crc_ok {
                    s.invalid_crc += 1;
                }
            }

            // Check sequence continuity
            let seq_hdr_count = envelope.ccsds_header.as_ref().map(|h| h.sequence_count).unwrap_or(0);
            if let Some(&last) = s.last_seq_count.get(&envelope.apid) {
                let expected = (last + 1) % 16384;
                if seq_hdr_count != expected {
                    s.sequence_gaps += 1;
                }
            }
            s.last_seq_count.insert(envelope.apid, seq_hdr_count);

            // Latency stats
            if latency < s.latency_min {
                s.latency_min = latency;
            }
            if latency > s.latency_max {
                s.latency_max = latency;
            }
            s.latency_sum += latency;
            s.latencies.push(latency);

            // Periodically flush and write summary in case of crash
            if s.total_received % 1000 == 0 {
                csv_writer.flush()?;
                let _ = write_summary(&s, &json_path);
            }
        }
    }

    Ok(())
}

fn write_summary(s: &RunStats, path: &str) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    
    // Sort latencies to compute percentiles
    let mut sorted_latencies = s.latencies.clone();
    sorted_latencies.sort_unstable();

    let count = sorted_latencies.len();
    let p50 = if count > 0 { sorted_latencies[count / 2] } else { 0 };
    let p95 = if count > 0 { sorted_latencies[(count as f64 * 0.95) as usize] } else { 0 };
    let p99 = if count > 0 { sorted_latencies[(count as f64 * 0.99) as usize] } else { 0 };
    let avg = if count > 0 { s.latency_sum / count as u64 } else { 0 };

    let mut apid_map = String::new();
    for (k, v) in &s.by_apid {
        if !apid_map.is_empty() {
            apid_map.push_str(", ");
        }
        apid_map.push_str(&format!("\"{}\": {}", k, v));
    }

    writeln!(file, "{{")?;
    writeln!(file, "  \"total_received\": {},", s.total_received)?;
    writeln!(file, "  \"by_apid\": {{{}}},", apid_map)?;
    writeln!(file, "  \"sequence_gaps\": {},", s.sequence_gaps)?;
    writeln!(file, "  \"invalid_crc\": {},", s.invalid_crc)?;
    writeln!(file, "  \"latency_ns\": {{")?;
    writeln!(file, "    \"min\": {},", if s.latency_min == u64::MAX { 0 } else { s.latency_min })?;
    writeln!(file, "    \"max\": {},", s.latency_max)?;
    writeln!(file, "    \"avg\": {},", avg)?;
    writeln!(file, "    \"p50\": {},", p50)?;
    writeln!(file, "    \"p95\": {},", p95)?;
    writeln!(file, "    \"p99\": {}", p99)?;
    writeln!(file, "  }}")?;
    writeln!(file, "}}")?;
    
    Ok(())
}

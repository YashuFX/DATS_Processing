// ── Console Outbound Sink ─────────────────────────────────────────────────────
//
// Responsibility: Format and log the decoded metadata summaries to stdout.
//
// This is the implementation of the driven port `DecodedSink`.
//
// In Hexagonal Architecture, this allows running the whole pipeline in local/test
// environments without needing a live RabbitMQ publisher.

use async_trait::async_trait;

use crate::domain::errors::DecoderError;
use crate::ports::outbound::{DecodeResult, DecodedSink};

#[derive(Debug, Default, Clone)]
pub struct ConsoleSink;

impl ConsoleSink {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DecodedSink for ConsoleSink {
    async fn emit(&self, r: &DecodeResult) -> Result<(), DecoderError> {
        let seq_flag = if r.is_gap {
            " \u{26A0} GAP" // ⚠
        } else if r.is_duplicate {
            " \u{26A0} DUP" // ⚠
        } else {
            ""
        };

        let crc_flag = if r.crc_ok { "CRC✓" } else { "CRC✗" };

        let time_str = if let Some(ref sec) = r.secondary_header {
            format!(
                "Time={}.{:03} ({})",
                sec.coarse_time, sec.fine_time, sec.format
            )
        } else {
            "Time=None".to_string()
        };

        tracing::info!(
            "[CCSDS ✓] {crc_flag} APID={apid:4} | Seq={seq:5}{seq_flag} | Ver={ver} | Type={pkt_type} | DataLen={dlen} B | {time_str} | ID={id}",
            crc_flag = crc_flag,
            apid = r.apid,
            seq  = r.seq_count,
            seq_flag = seq_flag,
            ver  = r.version,
            pkt_type = if r.packet_type == 0 { "TM" } else { "TC" },
            dlen = r.data_len_bytes,
            time_str = time_str,
            id   = if r.envelope_id.len() >= 8 { &r.envelope_id[..8] } else { &r.envelope_id },
        );

        Ok(())
    }
}

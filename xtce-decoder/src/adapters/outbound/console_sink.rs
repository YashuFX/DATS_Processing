use async_trait::async_trait;
use crate::ports::outbound::AlertPort;

pub struct ConsoleAlertPort;

#[async_trait]
impl AlertPort for ConsoleAlertPort {
    async fn emit_warning(&self, context: &str, message: &str) {
        tracing::warn!(context = %context, "XTCE Decoder Warning: {}", message);
    }

    async fn emit_critical(&self, context: &str, message: &str) {
        tracing::error!(context = %context, "XTCE Decoder Critical Error: {}", message);
    }
}

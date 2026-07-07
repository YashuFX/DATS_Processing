use std::sync::Mutex;
use crate::ports::EventPort;
use crate::api::events::v1::PlatformEvent;
use crate::domain::errors::ReplayError;

/// An adapter for EventPort that logs emitted platform events and stores them in memory.
pub struct LoggingEventPublisher {
    events: Mutex<Vec<PlatformEvent>>,
}

impl LoggingEventPublisher {
    /// Creates a new LoggingEventPublisher.
    pub fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }

    /// Gets a copy of all emitted events for testing purposes.
    pub fn get_emitted_events(&self) -> Vec<PlatformEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl EventPort for LoggingEventPublisher {
    fn emit(&self, event: PlatformEvent) -> Result<(), ReplayError> {
        match event.severity {
            3 | 4 => {
                tracing::error!(
                    "[Platform Event] TYPE: {} | SEVERITY: {} | MESSAGE: {}",
                    event.event_type, event.severity, event.message
                );
            }
            2 => {
                tracing::warn!(
                    "[Platform Event] TYPE: {} | SEVERITY: {} | MESSAGE: {}",
                    event.event_type, event.severity, event.message
                );
            }
            _ => {
                tracing::info!(
                    "[Platform Event] TYPE: {} | SEVERITY: {} | MESSAGE: {}",
                    event.event_type, event.severity, event.message
                );
            }
        }
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}

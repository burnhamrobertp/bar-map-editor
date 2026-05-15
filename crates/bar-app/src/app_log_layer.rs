//! Custom tracing subscriber layer that forwards INFO+ events to the BME log.

use std::sync::mpsc;

use tracing::Level;
use tracing_subscriber::Layer;

pub struct AppLogLayer {
    tx: mpsc::Sender<(Level, String)>,
}

impl AppLogLayer {
    pub fn new(tx: mpsc::Sender<(Level, String)>) -> Self {
        Self { tx }
    }
}

impl<S: tracing::Subscriber> Layer<S> for AppLogLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let level = *event.metadata().level();
        // Forward DEBUG and above. The BME log panel has its own
        // per-level visibility toggle (the "DEBUG" button) so anything
        // below INFO stays hidden by default but is available for
        // diagnostics. TRACE is still dropped here -- it's noisy and the
        // panel doesn't have a TRACE toggle.
        if level > Level::DEBUG {
            return;
        }
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let _ = self.tx.send((level, visitor.message));
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            if !self.message.is_empty() {
                self.message.push(' ');
            }
            self.message
                .push_str(&format!("{}={}", field.name(), value));
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            // tracing's fmt layer strips the surrounding quotes on string messages;
            // replicate that by formatting and stripping the outer quotes if present.
            let raw = format!("{:?}", value);
            self.message = if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
                raw[1..raw.len() - 1].to_string()
            } else {
                raw
            };
        } else {
            if !self.message.is_empty() {
                self.message.push(' ');
            }
            self.message
                .push_str(&format!("{}={:?}", field.name(), value));
        }
    }
}

//! Forwards `tracing` WARN/ERROR events from the RomMFS crates into the
//! durable diagnostic log. Without a subscriber those macros are silent, so
//! failures such as a deferred eviction or a failed hydration left no trace.
//! File only: these are support detail, not Activity items.

use rommfs_core::events::{EventSink, Level};
use std::fmt::Write as _;
use tracing::{
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    Event, Metadata, Subscriber,
};

pub struct TraceBridge {
    sink: EventSink,
}

impl TraceBridge {
    /// Install as the process-wide subscriber. A subscriber installed earlier
    /// (for example by a test harness) is left in place.
    pub fn install(sink: EventSink) {
        let _ = tracing::subscriber::set_global_default(Self { sink });
    }
}

/// Renders `message` first, then `key=value` pairs, on one line.
#[derive(Default)]
struct Rendered {
    message: String,
    fields: String,
}

impl Visit for Rendered {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={value:?}", field.name());
        }
    }
}

fn ours(target: &str) -> bool {
    target.starts_with("rommfs")
}

fn level_of(level: &tracing::Level) -> Option<Level> {
    if *level == tracing::Level::ERROR {
        Some(Level::Error)
    } else if *level == tracing::Level::WARN {
        Some(Level::Warn)
    } else {
        None
    }
}

impl Subscriber for TraceBridge {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        ours(metadata.target()) && level_of(metadata.level()).is_some()
    }
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let metadata = event.metadata();
        let Some(level) = level_of(metadata.level()).filter(|_| ours(metadata.target())) else {
            return;
        };
        let mut rendered = Rendered::default();
        event.record(&mut rendered);
        self.sink.log_detail(
            level,
            metadata.target(),
            format!("{}{}", rendered.message, rendered.fields),
        );
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_rommfs_warnings_and_errors_are_forwarded() {
        let (sink, _rx) = rommfs_core::events::channel();
        let dir = tempfile::tempdir().unwrap();
        let sink = sink.with_log_directory(dir.path()).unwrap();
        tracing::subscriber::with_default(TraceBridge { sink }, || {
            tracing::warn!(target: "rommfs_core::cache::policy", rom_id = 7, "eviction deferred");
            tracing::info!(target: "rommfs_core::cache::policy", "too chatty");
            tracing::warn!(target: "gpui::other", "foreign");
        });
        let text = std::fs::read_to_string(dir.path().join("rommfs.log")).unwrap();
        assert!(text.contains("Warn rommfs_core::cache::policy eviction deferred rom_id=7"));
        assert!(!text.contains("too chatty") && !text.contains("foreign"));
    }
}

//! Fine-grained parser callbacks for structured output events.
//!
//! Unlike the existing block-level callbacks in `streaming.rs`, these
//! provide token-level events for JSON keys, values, tag opens/closes,
//! grammar matches, and recovery attempts.

/// Events emitted by the structured parser during incremental parsing.
#[derive(Debug, Clone)]
pub enum ParserEvent {
    // ── JSON events ──────────────────────────────────────
    /// A complete JSON value has been parsed.
    JsonComplete {
        value: Option<serde_json::Value>,
    },
    /// A JSON key has been encountered.
    JsonKey {
        key: String,
    },
    /// A JSON value has been encountered (within an object or array).
    JsonValue {
        value: serde_json::Value,
    },

    // ── Tag/XML events ───────────────────────────────────
    /// An opening tag was detected.
    TagOpen {
        name: String,
    },
    /// A closing tag was detected.
    TagClose {
        name: String,
    },

    // ── Grammar events ───────────────────────────────────
    /// A grammar rule was matched.
    GrammarMatch {
        rule: String,
    },

    // ── Generic events ───────────────────────────────────
    /// Raw text delta (for passthrough or unstructured content).
    TextDelta {
        content: String,
    },
    /// A recovery action was taken after a structural error.
    RecoveryAttempt {
        action: String,
    },
    /// Parsing has completed (final event).
    Complete,
}

/// Trait for receiving parser events.
///
/// Implement this to build custom consumers that react to fine-grained
/// structural events as they stream in.  The event surface provides
/// equivalents of `on_json_start`, `on_tag_open`, etc.
pub trait ParserCallback: Send + Sync {
    fn on_event(&self, event: &ParserEvent);
}

/// A simple callback that collects events into a shared vector.
/// Useful for testing and debugging.
pub struct CollectingCallback {
    events: std::sync::Mutex<Vec<ParserEvent>>,
}

impl CollectingCallback {
    pub fn new() -> Self {
        Self {
            events: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn events(&self) -> Vec<ParserEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl Default for CollectingCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl ParserCallback for CollectingCallback {
    fn on_event(&self, event: &ParserEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}

/// A callback that logs events via `tracing`.
pub struct TracingCallback;

impl ParserCallback for TracingCallback {
    fn on_event(&self, event: &ParserEvent) {
        match event {
            ParserEvent::JsonComplete { .. } => tracing::debug!("JSON value complete"),
            ParserEvent::JsonKey { key } => tracing::trace!("JSON key: {key}"),
            ParserEvent::JsonValue { .. } => tracing::trace!("JSON value"),
            ParserEvent::TagOpen { name } => tracing::debug!("tag open: <{name}>"),
            ParserEvent::TagClose { name } => tracing::debug!("tag close: </{name}>"),
            ParserEvent::GrammarMatch { rule } => tracing::trace!("grammar match: {rule}"),
            ParserEvent::TextDelta { content } => {
                tracing::trace!("text delta: {} bytes", content.len())
            }
            ParserEvent::RecoveryAttempt { action } => {
                tracing::warn!("parser recovery: {action}")
            }
            ParserEvent::Complete => tracing::debug!("parsing complete"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collecting_callback_captures_events() {
        let cb = CollectingCallback::new();
        cb.on_event(&ParserEvent::TagOpen {
            name: "test".into(),
        });
        cb.on_event(&ParserEvent::TagClose {
            name: "test".into(),
        });
        let events = cb.events();
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], ParserEvent::TagOpen { name } if name == "test"));
        assert!(matches!(&events[1], ParserEvent::TagClose { name } if name == "test"));
    }

    #[test]
    fn tracing_callback_doesnt_panic() {
        let cb = TracingCallback;
        cb.on_event(&ParserEvent::Complete);
        cb.on_event(&ParserEvent::JsonComplete { value: None });
        cb.on_event(&ParserEvent::RecoveryAttempt {
            action: "skip".into(),
        });
    }
}

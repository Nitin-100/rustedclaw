//! Audit logging — structured security event logging.
//!
//! Records security-relevant events for monitoring and compliance.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub event: AuditEvent,
    pub actor: String,
    pub target: String,
    pub outcome: AuditOutcome,
    pub details: Option<String>,
}

/// Types of auditable security events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuditEvent {
    /// Pairing attempt from a user
    PairAttempt,
    /// Authentication/authorization failure
    AuthFailure,
    /// Tool was executed
    ToolExecution { tool_name: String },
    /// Configuration was changed
    ConfigChange { key: String },
    /// A sender was blocked by allowlist
    SenderBlocked { channel: String },
    /// Path access was denied
    PathDenied { path: String },
    /// Endpoint access was denied (SSRF prevention)
    EndpointDenied { url: String },
    /// Secret was accessed
    SecretAccess { secret_id: String },
}

/// Outcome of an audited operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    Success,
    Failure,
    Denied,
}

/// Trait for audit log sinks (where events are written).
pub trait AuditSink: Send + Sync {
    fn record(&self, entry: &AuditEntry);
}

/// In-memory audit logger that stores entries in a vector.
/// Useful for testing and small deployments.
pub struct AuditLogger {
    entries: std::sync::Mutex<Vec<AuditEntry>>,
    sinks: Vec<Box<dyn AuditSink>>,
}

impl std::fmt::Debug for AuditLogger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.entries.lock().unwrap().len();
        f.debug_struct("AuditLogger")
            .field("entry_count", &count)
            .field("sink_count", &self.sinks.len())
            .finish()
    }
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditLogger {
    /// Create a new audit logger with no sinks.
    pub fn new() -> Self {
        Self {
            entries: std::sync::Mutex::new(Vec::new()),
            sinks: Vec::new(),
        }
    }

    /// Create a new audit logger with the given sinks.
    pub fn with_sinks(sinks: Vec<Box<dyn AuditSink>>) -> Self {
        Self {
            entries: std::sync::Mutex::new(Vec::new()),
            sinks,
        }
    }

    /// Record an audit event.
    pub fn log(&self, event: AuditEvent, actor: &str, target: &str, outcome: AuditOutcome, details: Option<String>) {
        let entry = AuditEntry {
            timestamp: Utc::now(),
            event,
            actor: actor.into(),
            target: target.into(),
            outcome,
            details,
        };

        // Store in memory
        self.entries.lock().unwrap().push(entry.clone());

        // Forward to sinks
        for sink in &self.sinks {
            sink.record(&entry);
        }
    }

    /// Get all recorded entries.
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.entries.lock().unwrap().clone()
    }

    /// Get entries filtered by event type.
    pub fn entries_by_outcome(&self, outcome: &AuditOutcome) -> Vec<AuditEntry> {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .filter(|e| &e.outcome == outcome)
            .cloned()
            .collect()
    }

    /// Clear all stored entries.
    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }

    /// Count of stored entries.
    pub fn count(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

/// A tracing-based audit sink that logs entries via `tracing::info!`.
pub struct TracingSink;

impl AuditSink for TracingSink {
    fn record(&self, entry: &AuditEntry) {
        tracing::info!(
            event = ?entry.event,
            actor = %entry.actor,
            target = %entry.target,
            outcome = ?entry.outcome,
            details = ?entry.details,
            "AUDIT"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_and_retrieve_entries() {
        let logger = AuditLogger::new();
        logger.log(
            AuditEvent::PairAttempt,
            "user@test",
            "system",
            AuditOutcome::Success,
            None,
        );
        logger.log(
            AuditEvent::AuthFailure,
            "attacker",
            "system",
            AuditOutcome::Denied,
            Some("wrong token".into()),
        );

        assert_eq!(logger.count(), 2);
        let entries = logger.entries();
        assert_eq!(entries[0].actor, "user@test");
        assert_eq!(entries[1].actor, "attacker");
    }

    #[test]
    fn filter_by_outcome() {
        let logger = AuditLogger::new();
        logger.log(
            AuditEvent::PairAttempt,
            "user1",
            "system",
            AuditOutcome::Success,
            None,
        );
        logger.log(
            AuditEvent::AuthFailure,
            "user2",
            "system",
            AuditOutcome::Denied,
            None,
        );
        logger.log(
            AuditEvent::ToolExecution { tool_name: "shell".into() },
            "user1",
            "shell_tool",
            AuditOutcome::Success,
            None,
        );

        let successes = logger.entries_by_outcome(&AuditOutcome::Success);
        assert_eq!(successes.len(), 2);

        let denied = logger.entries_by_outcome(&AuditOutcome::Denied);
        assert_eq!(denied.len(), 1);
        assert_eq!(denied[0].actor, "user2");
    }

    #[test]
    fn clear_entries() {
        let logger = AuditLogger::new();
        logger.log(
            AuditEvent::PairAttempt,
            "user",
            "system",
            AuditOutcome::Success,
            None,
        );
        assert_eq!(logger.count(), 1);
        logger.clear();
        assert_eq!(logger.count(), 0);
    }

    #[test]
    fn audit_entry_serialization() {
        let entry = AuditEntry {
            timestamp: Utc::now(),
            event: AuditEvent::ToolExecution { tool_name: "file_read".into() },
            actor: "agent".into(),
            target: "/home/user/file.txt".into(),
            outcome: AuditOutcome::Success,
            details: Some("read 42 bytes".into()),
        };

        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: AuditEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.actor, "agent");
        assert_eq!(deserialized.outcome, AuditOutcome::Success);
    }

    #[test]
    fn audit_event_variants_serialize() {
        let events = vec![
            AuditEvent::PairAttempt,
            AuditEvent::AuthFailure,
            AuditEvent::ToolExecution { tool_name: "shell".into() },
            AuditEvent::ConfigChange { key: "provider".into() },
            AuditEvent::SenderBlocked { channel: "discord".into() },
            AuditEvent::PathDenied { path: "/etc/passwd".into() },
            AuditEvent::EndpointDenied { url: "http://169.254.169.254".into() },
            AuditEvent::SecretAccess { secret_id: "api_key".into() },
        ];

        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            let round_tripped: AuditEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(round_tripped, event);
        }
    }

    #[test]
    fn custom_sink_receives_events() {
        use std::sync::{Arc, Mutex};

        struct TestSink {
            received: Arc<Mutex<Vec<String>>>,
        }

        impl AuditSink for TestSink {
            fn record(&self, entry: &AuditEntry) {
                self.received.lock().unwrap().push(entry.actor.clone());
            }
        }

        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = TestSink { received: received.clone() };
        let logger = AuditLogger::with_sinks(vec![Box::new(sink)]);

        logger.log(
            AuditEvent::PairAttempt,
            "user@test",
            "system",
            AuditOutcome::Success,
            None,
        );

        let sink_entries = received.lock().unwrap();
        assert_eq!(sink_entries.len(), 1);
        assert_eq!(sink_entries[0], "user@test");
    }

    #[test]
    fn default_logger() {
        let logger = AuditLogger::default();
        assert_eq!(logger.count(), 0);
    }

    #[test]
    fn debug_format() {
        let logger = AuditLogger::new();
        let debug_str = format!("{logger:?}");
        assert!(debug_str.contains("AuditLogger"));
        assert!(debug_str.contains("entry_count"));
    }
}

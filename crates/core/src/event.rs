//! Domain event system — decoupled communication between bounded contexts.
//!
//! Events are published when something interesting happens in the system.
//! Other components can subscribe to react without tight coupling.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;

/// All domain events in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DomainEvent {
    /// A new message was received from a channel
    MessageReceived {
        channel: String,
        sender_id: String,
        content_preview: String,
        timestamp: DateTime<Utc>,
    },

    /// The agent generated a response
    ResponseGenerated {
        conversation_id: String,
        model: String,
        tokens_used: u32,
        timestamp: DateTime<Utc>,
    },

    /// A tool was executed
    ToolExecuted {
        tool_name: String,
        success: bool,
        duration_ms: u64,
        timestamp: DateTime<Utc>,
    },

    /// A memory was stored or retrieved
    MemoryAccessed {
        operation: String, // "store", "search", "delete"
        count: usize,
        timestamp: DateTime<Utc>,
    },

    /// An error occurred
    ErrorOccurred {
        context: String,
        error_message: String,
        timestamp: DateTime<Utc>,
    },

    /// Agent state changed
    AgentStateChanged {
        is_busy: bool,
        requests_processed: u64,
        timestamp: DateTime<Utc>,
    },
}

/// A broadcast-based event bus for domain events.
///
/// Uses `tokio::sync::broadcast` for multi-consumer pub/sub.
/// Components can subscribe to receive all events and filter for what they care about.
pub struct EventBus {
    sender: broadcast::Sender<Arc<DomainEvent>>,
}

impl EventBus {
    /// Create a new event bus with the given capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Publish an event to all subscribers.
    pub fn publish(&self, event: DomainEvent) {
        // Ignore send errors (no subscribers = that's fine)
        let _ = self.sender.send(Arc::new(event));
    }

    /// Subscribe to receive events.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<DomainEvent>> {
        self.sender.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn event_bus_publish_subscribe() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();

        bus.publish(DomainEvent::ToolExecuted {
            tool_name: "shell".into(),
            success: true,
            duration_ms: 42,
            timestamp: Utc::now(),
        });

        let event = rx.recv().await.unwrap();
        match event.as_ref() {
            DomainEvent::ToolExecuted { tool_name, success, .. } => {
                assert_eq!(tool_name, "shell");
                assert!(success);
            }
            _ => panic!("Expected ToolExecuted event"),
        }
    }

    #[test]
    fn event_bus_no_subscribers_doesnt_panic() {
        let bus = EventBus::new(16);
        // Publishing with no subscribers should not panic
        bus.publish(DomainEvent::ErrorOccurred {
            context: "test".into(),
            error_message: "no subscribers".into(),
            timestamp: Utc::now(),
        });
    }
}

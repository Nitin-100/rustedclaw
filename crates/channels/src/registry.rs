//! Channel registry — manages all active channel instances.
//!
//! Routes inbound messages from channels to the agent and dispatches
//! outbound responses back to the correct channel.

use std::collections::HashMap;
use std::sync::Arc;

use rustedclaw_core::channel::{Channel, ChannelMessage};
use rustedclaw_core::error::ChannelError;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Central registry holding all enabled channel instances.
pub struct ChannelRegistry {
    channels: HashMap<String, Arc<dyn Channel>>,
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            channels: HashMap::new(),
        }
    }

    /// Register a channel adapter.
    pub fn register(&mut self, channel: Arc<dyn Channel>) {
        let name = channel.name().to_string();
        info!(channel = %name, "Registered channel");
        self.channels.insert(name, channel);
    }

    /// Get a channel by name.
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Channel>> {
        self.channels.get(name)
    }

    /// List all registered channel names.
    pub fn list(&self) -> Vec<String> {
        self.channels.keys().cloned().collect()
    }

    /// Number of registered channels.
    pub fn len(&self) -> usize {
        self.channels.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
    }

    /// Start all channels and merge their message streams into one receiver.
    pub async fn start_all(
        &self,
    ) -> Result<mpsc::Receiver<(String, Result<ChannelMessage, ChannelError>)>, ChannelError> {
        let (merged_tx, merged_rx) = mpsc::channel(256);

        for (name, channel) in &self.channels {
            let rx = channel.start().await?;
            let tx = merged_tx.clone();
            let channel_name = name.clone();

            tokio::spawn(async move {
                let mut rx = rx;
                while let Some(msg) = rx.recv().await {
                    if tx.send((channel_name.clone(), msg)).await.is_err() {
                        break; // Merged receiver dropped
                    }
                }
            });

            info!(channel = %name, "Started channel");
        }

        Ok(merged_rx)
    }

    /// Send a message to a specific channel.
    pub async fn send_to(
        &self,
        channel_name: &str,
        chat_id: &str,
        content: &str,
        reply_to: Option<&str>,
    ) -> Result<(), ChannelError> {
        let channel = self.channels.get(channel_name).ok_or_else(|| {
            ChannelError::NotConfigured(format!("Channel '{}' not found", channel_name))
        })?;

        channel.send(chat_id, content, reply_to).await
    }

    /// Stop all channels gracefully.
    pub async fn stop_all(&self) {
        for (name, channel) in &self.channels {
            if let Err(e) = channel.stop().await {
                warn!(channel = %name, error = %e, "Failed to stop channel");
            }
        }
    }

    /// Run health checks on all channels.
    pub async fn health_check_all(&self) -> HashMap<String, bool> {
        let mut results = HashMap::new();
        for (name, channel) in &self.channels {
            let healthy = channel.health_check().await.unwrap_or(false);
            results.insert(name.clone(), healthy);
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rustedclaw_core::channel::ChannelId;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct MockChannel {
        name: String,
        channel_id: ChannelId,
        allowed: Vec<String>,
        started: AtomicBool,
        stopped: AtomicBool,
    }

    impl MockChannel {
        fn new(name: &str) -> Self {
            Self {
                name: name.into(),
                channel_id: ChannelId(name.into()),
                allowed: vec!["*".into()],
                started: AtomicBool::new(false),
                stopped: AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl Channel for MockChannel {
        fn name(&self) -> &str { &self.name }
        fn id(&self) -> &ChannelId { &self.channel_id }

        async fn start(&self) -> Result<mpsc::Receiver<Result<ChannelMessage, ChannelError>>, ChannelError> {
            self.started.store(true, Ordering::SeqCst);
            let (_tx, rx) = mpsc::channel(1);
            Ok(rx)
        }

        async fn send(&self, _chat_id: &str, _content: &str, _reply_to: Option<&str>) -> Result<(), ChannelError> {
            Ok(())
        }

        fn is_allowed(&self, sender_id: &str) -> bool {
            self.allowed.contains(&"*".to_string()) || self.allowed.contains(&sender_id.to_string())
        }

        async fn stop(&self) -> Result<(), ChannelError> {
            self.stopped.store(true, Ordering::SeqCst);
            Ok(())
        }

        async fn health_check(&self) -> Result<bool, ChannelError> {
            Ok(self.started.load(Ordering::SeqCst))
        }
    }

    #[test]
    fn empty_registry() {
        let reg = ChannelRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn register_and_list() {
        let mut reg = ChannelRegistry::new();
        reg.register(Arc::new(MockChannel::new("telegram")));
        reg.register(Arc::new(MockChannel::new("discord")));

        assert_eq!(reg.len(), 2);
        assert!(reg.list().contains(&"telegram".to_string()));
        assert!(reg.list().contains(&"discord".to_string()));
    }

    #[test]
    fn get_channel() {
        let mut reg = ChannelRegistry::new();
        reg.register(Arc::new(MockChannel::new("telegram")));

        assert!(reg.get("telegram").is_some());
        assert!(reg.get("slack").is_none());
    }

    #[tokio::test]
    async fn start_all_channels() {
        let mut reg = ChannelRegistry::new();
        let ch = Arc::new(MockChannel::new("test"));
        reg.register(ch.clone());

        let _rx = reg.start_all().await.unwrap();
        assert!(ch.started.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn stop_all_channels() {
        let mut reg = ChannelRegistry::new();
        let ch = Arc::new(MockChannel::new("test"));
        reg.register(ch.clone());

        reg.stop_all().await;
        assert!(ch.stopped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn health_check_all() {
        let mut reg = ChannelRegistry::new();
        let ch = Arc::new(MockChannel::new("test"));
        reg.register(ch.clone());

        // Not started yet
        let health = reg.health_check_all().await;
        assert_eq!(health.get("test"), Some(&false));

        // Start
        let _rx = reg.start_all().await.unwrap();
        let health = reg.health_check_all().await;
        assert_eq!(health.get("test"), Some(&true));
    }

    #[tokio::test]
    async fn send_to_channel() {
        let mut reg = ChannelRegistry::new();
        reg.register(Arc::new(MockChannel::new("test")));

        let result = reg.send_to("test", "chat1", "Hello", None).await;
        assert!(result.is_ok());

        let result = reg.send_to("nonexistent", "chat1", "Hello", None).await;
        assert!(result.is_err());
    }
}

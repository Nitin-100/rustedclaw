//! Slack channel adapter (stub).
//!
//! Implements the Channel trait for Slack using Socket Mode.
//! In production, this would connect via WebSocket to Slack's Socket Mode API,
//! requiring no public URL. Currently a stub with in-process injection.

use async_trait::async_trait;
use rustedclaw_core::channel::{Channel, ChannelId, ChannelMessage};
use rustedclaw_core::error::ChannelError;
use tokio::sync::mpsc;
use tracing::info;

/// Slack channel configuration.
#[derive(Debug, Clone)]
pub struct SlackConfig {
    /// Bot token (xoxb-...).
    pub bot_token: String,
    /// App-level token (xapp-...) for Socket Mode.
    pub app_token: String,
    /// Allowed member IDs. Empty = deny all, ["*"] = allow all.
    pub allowed_users: Vec<String>,
}

/// Slack channel adapter.
pub struct SlackChannel {
    config: SlackConfig,
    channel_id: ChannelId,
    inject_tx: tokio::sync::Mutex<Option<mpsc::Sender<Result<ChannelMessage, ChannelError>>>>,
}

impl SlackChannel {
    pub fn new(config: SlackConfig) -> Self {
        Self {
            config,
            channel_id: ChannelId("slack".into()),
            inject_tx: tokio::sync::Mutex::new(None),
        }
    }

    /// Inject a message as if it came from Slack (for testing).
    pub async fn inject_message(&self, msg: ChannelMessage) -> Result<(), ChannelError> {
        let guard = self.inject_tx.lock().await;
        if let Some(tx) = guard.as_ref() {
            tx.send(Ok(msg)).await.map_err(|_| {
                ChannelError::ConnectionLost("Message channel closed".into())
            })
        } else {
            Err(ChannelError::ConnectionLost("Channel not started".into()))
        }
    }
}

#[async_trait]
impl Channel for SlackChannel {
    fn name(&self) -> &str { "slack" }

    fn id(&self) -> &ChannelId { &self.channel_id }

    async fn start(&self) -> Result<mpsc::Receiver<Result<ChannelMessage, ChannelError>>, ChannelError> {
        info!("Slack channel starting (stub mode — Socket Mode)");
        let (tx, rx) = mpsc::channel(64);
        *self.inject_tx.lock().await = Some(tx);
        Ok(rx)
    }

    async fn send(&self, chat_id: &str, content: &str, reply_to: Option<&str>) -> Result<(), ChannelError> {
        info!(
            chat_id = %chat_id,
            reply_to = ?reply_to,
            content_len = content.len(),
            "Slack send (stub — would POST to chat.postMessage)"
        );
        Ok(())
    }

    fn is_allowed(&self, sender_id: &str) -> bool {
        if self.config.allowed_users.is_empty() {
            return false;
        }
        if self.config.allowed_users.iter().any(|u| u == "*") {
            return true;
        }
        self.config.allowed_users.iter().any(|u| u == sender_id)
    }

    async fn stop(&self) -> Result<(), ChannelError> {
        info!("Slack channel stopping");
        *self.inject_tx.lock().await = None;
        Ok(())
    }

    async fn health_check(&self) -> Result<bool, ChannelError> {
        // In production: call auth.test API
        Ok(!self.config.bot_token.is_empty() && !self.config.app_token.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> SlackConfig {
        SlackConfig {
            bot_token: "xoxb-test-token".into(),
            app_token: "xapp-test-token".into(),
            allowed_users: vec!["*".into()],
        }
    }

    #[test]
    fn channel_name() {
        let ch = SlackChannel::new(test_config());
        assert_eq!(ch.name(), "slack");
    }

    #[test]
    fn allowlist() {
        let specific = SlackChannel::new(SlackConfig {
            allowed_users: vec!["U123".into(), "U456".into()],
            ..test_config()
        });
        assert!(specific.is_allowed("U123"));
        assert!(!specific.is_allowed("U999"));
    }

    #[tokio::test]
    async fn start_inject_receive() {
        let ch = SlackChannel::new(test_config());
        let mut rx = ch.start().await.unwrap();

        let msg = ChannelMessage {
            channel_id: ChannelId("slack".into()),
            sender_id: "U123".into(),
            sender_name: Some("Charlie".into()),
            content: "Hello from Slack!".into(),
            chat_id: "C789".into(),
            reply_to_message_id: None,
            attachments: vec![],
            metadata: serde_json::Map::new(),
        };

        ch.inject_message(msg).await.unwrap();
        let received = rx.recv().await.unwrap().unwrap();
        assert_eq!(received.content, "Hello from Slack!");
    }

    #[tokio::test]
    async fn health_check() {
        let ch = SlackChannel::new(test_config());
        assert!(ch.health_check().await.unwrap());

        let bad = SlackChannel::new(SlackConfig {
            bot_token: "".into(),
            app_token: "xapp-test".into(),
            allowed_users: vec![],
        });
        assert!(!bad.health_check().await.unwrap());
    }
}

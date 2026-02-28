//! Telegram channel adapter (stub).
//!
//! Implements the Channel trait for Telegram Bot API.
//! In production, this would use `teloxide` for long-polling or webhook mode.
//! Currently a stub that can receive/send messages via an in-process channel.

use async_trait::async_trait;
use rustedclaw_core::channel::{Channel, ChannelId, ChannelMessage};
use rustedclaw_core::error::ChannelError;
use tokio::sync::mpsc;
use tracing::info;

/// Telegram channel configuration.
#[derive(Clone)]
pub struct TelegramConfig {
    /// Bot token from @BotFather.
    pub bot_token: String,
    /// Allowed user IDs or usernames. Empty = deny all, ["*"] = allow all.
    pub allowed_users: Vec<String>,
    /// Whether to use webhook mode instead of long polling.
    pub use_webhook: bool,
}

impl std::fmt::Debug for TelegramConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelegramConfig")
            .field("bot_token", &"[REDACTED]")
            .field("allowed_users", &self.allowed_users)
            .field("use_webhook", &self.use_webhook)
            .finish()
    }
}

/// Telegram channel adapter.
pub struct TelegramChannel {
    config: TelegramConfig,
    channel_id: ChannelId,
    /// Sender for injecting test messages.
    inject_tx: tokio::sync::Mutex<Option<mpsc::Sender<Result<ChannelMessage, ChannelError>>>>,
}

impl TelegramChannel {
    pub fn new(config: TelegramConfig) -> Self {
        Self {
            config,
            channel_id: ChannelId("telegram".into()),
            inject_tx: tokio::sync::Mutex::new(None),
        }
    }

    /// Inject a message as if it came from Telegram (for testing).
    pub async fn inject_message(&self, msg: ChannelMessage) -> Result<(), ChannelError> {
        let guard = self.inject_tx.lock().await;
        if let Some(tx) = guard.as_ref() {
            tx.send(Ok(msg))
                .await
                .map_err(|_| ChannelError::ConnectionLost("Message channel closed".into()))
        } else {
            Err(ChannelError::ConnectionLost("Channel not started".into()))
        }
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    fn id(&self) -> &ChannelId {
        &self.channel_id
    }

    async fn start(
        &self,
    ) -> Result<mpsc::Receiver<Result<ChannelMessage, ChannelError>>, ChannelError> {
        info!("Telegram channel starting (stub mode)");
        let (tx, rx) = mpsc::channel(64);
        *self.inject_tx.lock().await = Some(tx);
        // In production: spawn teloxide long-polling loop here
        Ok(rx)
    }

    async fn send(
        &self,
        chat_id: &str,
        content: &str,
        reply_to: Option<&str>,
    ) -> Result<(), ChannelError> {
        info!(
            chat_id = %chat_id,
            reply_to = ?reply_to,
            content_len = content.len(),
            "Telegram send (stub)"
        );
        // In production: call Bot::send_message via teloxide
        Ok(())
    }

    async fn send_typing(&self, chat_id: &str) -> Result<(), ChannelError> {
        info!(chat_id = %chat_id, "Telegram typing (stub)");
        // In production: call sendChatAction with "typing"
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
        info!("Telegram channel stopping");
        *self.inject_tx.lock().await = None;
        Ok(())
    }

    async fn health_check(&self) -> Result<bool, ChannelError> {
        // In production: call getMe API
        Ok(!self.config.bot_token.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> TelegramConfig {
        TelegramConfig {
            bot_token: "test-token-123".into(),
            allowed_users: vec!["*".into()],
            use_webhook: false,
        }
    }

    #[test]
    fn channel_name_and_id() {
        let ch = TelegramChannel::new(test_config());
        assert_eq!(ch.name(), "telegram");
        assert_eq!(ch.id().0, "telegram");
    }

    #[test]
    fn allowlist_wildcard() {
        let ch = TelegramChannel::new(test_config());
        assert!(ch.is_allowed("anyone"));
    }

    #[test]
    fn allowlist_specific() {
        let ch = TelegramChannel::new(TelegramConfig {
            bot_token: "tok".into(),
            allowed_users: vec!["alice".into(), "bob".into()],
            use_webhook: false,
        });
        assert!(ch.is_allowed("alice"));
        assert!(ch.is_allowed("bob"));
        assert!(!ch.is_allowed("eve"));
    }

    #[test]
    fn allowlist_empty_denies() {
        let ch = TelegramChannel::new(TelegramConfig {
            bot_token: "tok".into(),
            allowed_users: vec![],
            use_webhook: false,
        });
        assert!(!ch.is_allowed("anyone"));
    }

    #[tokio::test]
    async fn start_and_inject() {
        let ch = TelegramChannel::new(test_config());
        let mut rx = ch.start().await.unwrap();

        let msg = ChannelMessage {
            channel_id: ChannelId("telegram".into()),
            sender_id: "user123".into(),
            sender_name: Some("Alice".into()),
            content: "Hello bot!".into(),
            chat_id: "chat456".into(),
            reply_to_message_id: None,
            attachments: vec![],
            metadata: serde_json::Map::new(),
        };

        ch.inject_message(msg).await.unwrap();

        let received = rx.recv().await.unwrap().unwrap();
        assert_eq!(received.content, "Hello bot!");
        assert_eq!(received.sender_id, "user123");
    }

    #[tokio::test]
    async fn send_stub() {
        let ch = TelegramChannel::new(test_config());
        let result = ch.send("chat1", "Hello!", None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn health_check() {
        let ch = TelegramChannel::new(test_config());
        assert!(ch.health_check().await.unwrap());

        let empty = TelegramChannel::new(TelegramConfig {
            bot_token: "".into(),
            allowed_users: vec![],
            use_webhook: false,
        });
        assert!(!empty.health_check().await.unwrap());
    }

    #[tokio::test]
    async fn stop_channel() {
        let ch = TelegramChannel::new(test_config());
        let _rx = ch.start().await.unwrap();
        ch.stop().await.unwrap();
        // Inject should fail after stop
        let msg = ChannelMessage {
            channel_id: ChannelId("telegram".into()),
            sender_id: "user".into(),
            sender_name: None,
            content: "test".into(),
            chat_id: "chat".into(),
            reply_to_message_id: None,
            attachments: vec![],
            metadata: serde_json::Map::new(),
        };
        assert!(ch.inject_message(msg).await.is_err());
    }
}

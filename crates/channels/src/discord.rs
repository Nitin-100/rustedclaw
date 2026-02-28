//! Discord channel adapter (stub).
//!
//! Implements the Channel trait for Discord Bot API.
//! In production, this would use `serenity` for WebSocket gateway.
//! Currently a stub with in-process message injection for testing.

use async_trait::async_trait;
use rustedclaw_core::channel::{Channel, ChannelId, ChannelMessage};
use rustedclaw_core::error::ChannelError;
use tokio::sync::mpsc;
use tracing::info;

/// Discord channel configuration.
#[derive(Clone)]
pub struct DiscordConfig {
    /// Bot token from Discord Developer Portal.
    pub bot_token: String,
    /// Allowed user IDs. Empty = deny all, ["*"] = allow all.
    pub allowed_users: Vec<String>,
    /// Guild (server) IDs to listen in. Empty = all guilds.
    pub guild_filter: Vec<String>,
    /// Channel IDs to listen in. Empty = all channels.
    pub channel_filter: Vec<String>,
}

impl std::fmt::Debug for DiscordConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscordConfig")
            .field("bot_token", &"[REDACTED]")
            .field("allowed_users", &self.allowed_users)
            .field("guild_filter", &self.guild_filter)
            .field("channel_filter", &self.channel_filter)
            .finish()
    }
}

/// Discord channel adapter.
pub struct DiscordChannel {
    config: DiscordConfig,
    channel_id: ChannelId,
    inject_tx: tokio::sync::Mutex<Option<mpsc::Sender<Result<ChannelMessage, ChannelError>>>>,
}

impl DiscordChannel {
    pub fn new(config: DiscordConfig) -> Self {
        Self {
            config,
            channel_id: ChannelId("discord".into()),
            inject_tx: tokio::sync::Mutex::new(None),
        }
    }

    /// Inject a message as if it came from Discord (for testing).
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
impl Channel for DiscordChannel {
    fn name(&self) -> &str {
        "discord"
    }

    fn id(&self) -> &ChannelId {
        &self.channel_id
    }

    async fn start(
        &self,
    ) -> Result<mpsc::Receiver<Result<ChannelMessage, ChannelError>>, ChannelError> {
        info!("Discord channel starting (stub mode)");
        let (tx, rx) = mpsc::channel(64);
        *self.inject_tx.lock().await = Some(tx);
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
            "Discord send (stub)"
        );
        Ok(())
    }

    async fn send_typing(&self, chat_id: &str) -> Result<(), ChannelError> {
        info!(chat_id = %chat_id, "Discord typing (stub)");
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
        info!("Discord channel stopping");
        *self.inject_tx.lock().await = None;
        Ok(())
    }

    async fn health_check(&self) -> Result<bool, ChannelError> {
        Ok(!self.config.bot_token.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> DiscordConfig {
        DiscordConfig {
            bot_token: "test-discord-token".into(),
            allowed_users: vec!["*".into()],
            guild_filter: vec![],
            channel_filter: vec![],
        }
    }

    #[test]
    fn channel_name_and_id() {
        let ch = DiscordChannel::new(test_config());
        assert_eq!(ch.name(), "discord");
        assert_eq!(ch.id().0, "discord");
    }

    #[test]
    fn allowlist_checks() {
        let ch = DiscordChannel::new(test_config());
        assert!(ch.is_allowed("anyone"));

        let specific = DiscordChannel::new(DiscordConfig {
            allowed_users: vec!["user1".into()],
            ..test_config()
        });
        assert!(specific.is_allowed("user1"));
        assert!(!specific.is_allowed("user2"));

        let deny_all = DiscordChannel::new(DiscordConfig {
            allowed_users: vec![],
            ..test_config()
        });
        assert!(!deny_all.is_allowed("anyone"));
    }

    #[tokio::test]
    async fn start_inject_and_receive() {
        let ch = DiscordChannel::new(test_config());
        let mut rx = ch.start().await.unwrap();

        let msg = ChannelMessage {
            channel_id: ChannelId("discord".into()),
            sender_id: "user456".into(),
            sender_name: Some("Bob".into()),
            content: "Hey from Discord!".into(),
            chat_id: "guild#channel".into(),
            reply_to_message_id: None,
            attachments: vec![],
            metadata: serde_json::Map::new(),
        };

        ch.inject_message(msg).await.unwrap();
        let received = rx.recv().await.unwrap().unwrap();
        assert_eq!(received.content, "Hey from Discord!");
    }

    #[tokio::test]
    async fn send_and_health() {
        let ch = DiscordChannel::new(test_config());
        assert!(ch.send("channel1", "Hello!", None).await.is_ok());
        assert!(ch.health_check().await.unwrap());
    }
}

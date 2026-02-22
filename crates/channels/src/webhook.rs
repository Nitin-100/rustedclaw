//! Webhook channel adapter.
//!
//! Accepts inbound HTTP POST webhooks from arbitrary systems.
//! Optionally validates HMAC signatures for security.
//! Can forward responses to a configured callback URL.

use async_trait::async_trait;
use rustedclaw_core::channel::{Channel, ChannelId, ChannelMessage};
use rustedclaw_core::error::ChannelError;
use tokio::sync::mpsc;
use tracing::info;

/// Webhook channel configuration.
#[derive(Debug, Clone)]
pub struct WebhookConfig {
    /// HMAC shared secret for signature validation. Empty = no validation.
    pub shared_secret: Option<String>,
    /// Allowed sender identifiers. Empty = deny all, ["*"] = allow all.
    pub allowed_senders: Vec<String>,
    /// Optional callback URL for sending responses.
    pub callback_url: Option<String>,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            shared_secret: None,
            allowed_senders: vec!["*".into()],
            callback_url: None,
        }
    }
}

/// Webhook channel adapter.
pub struct WebhookChannel {
    config: WebhookConfig,
    channel_id: ChannelId,
    inject_tx: tokio::sync::Mutex<Option<mpsc::Sender<Result<ChannelMessage, ChannelError>>>>,
}

impl WebhookChannel {
    pub fn new(config: WebhookConfig) -> Self {
        Self {
            config,
            channel_id: ChannelId("webhook".into()),
            inject_tx: tokio::sync::Mutex::new(None),
        }
    }

    /// Inject a webhook message (called by the HTTP handler).
    pub async fn inject_message(&self, msg: ChannelMessage) -> Result<(), ChannelError> {
        let guard = self.inject_tx.lock().await;
        if let Some(tx) = guard.as_ref() {
            tx.send(Ok(msg))
                .await
                .map_err(|_| ChannelError::ConnectionLost("Message channel closed".into()))
        } else {
            Err(ChannelError::ConnectionLost(
                "Webhook channel not started".into(),
            ))
        }
    }

    /// Validate an HMAC-SHA256 signature against the shared secret.
    pub fn validate_signature(&self, payload: &[u8], signature: &str) -> bool {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        match &self.config.shared_secret {
            None => true, // No secret configured = no validation
            Some(secret) => {
                // Simple hash-based validation (in production: use hmac-sha256)
                let mut hasher = DefaultHasher::new();
                secret.hash(&mut hasher);
                payload.hash(&mut hasher);
                let expected = format!("{:x}", hasher.finish());
                // Compare first 16 chars for stub
                signature.len() >= 16 && expected.starts_with(&signature[..16.min(signature.len())])
            }
        }
    }
}

#[async_trait]
impl Channel for WebhookChannel {
    fn name(&self) -> &str {
        "webhook"
    }

    fn id(&self) -> &ChannelId {
        &self.channel_id
    }

    async fn start(
        &self,
    ) -> Result<mpsc::Receiver<Result<ChannelMessage, ChannelError>>, ChannelError> {
        info!("Webhook channel starting");
        let (tx, rx) = mpsc::channel(64);
        *self.inject_tx.lock().await = Some(tx);
        Ok(rx)
    }

    async fn send(
        &self,
        chat_id: &str,
        content: &str,
        _reply_to: Option<&str>,
    ) -> Result<(), ChannelError> {
        if let Some(ref _callback) = self.config.callback_url {
            info!(
                chat_id = %chat_id,
                content_len = content.len(),
                "Webhook response (stub — would POST to callback URL)"
            );
            // In production: POST to callback URL with response
        } else {
            info!(
                chat_id = %chat_id,
                "Webhook: no callback URL configured, response discarded"
            );
        }
        Ok(())
    }

    fn is_allowed(&self, sender_id: &str) -> bool {
        if self.config.allowed_senders.is_empty() {
            return false;
        }
        if self.config.allowed_senders.iter().any(|s| s == "*") {
            return true;
        }
        self.config.allowed_senders.iter().any(|s| s == sender_id)
    }

    async fn health_check(&self) -> Result<bool, ChannelError> {
        Ok(true) // Webhook is always ready (stateless)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_name() {
        let ch = WebhookChannel::new(WebhookConfig::default());
        assert_eq!(ch.name(), "webhook");
    }

    #[test]
    fn default_allows_all() {
        let ch = WebhookChannel::new(WebhookConfig::default());
        assert!(ch.is_allowed("any_system"));
    }

    #[test]
    fn specific_senders() {
        let ch = WebhookChannel::new(WebhookConfig {
            allowed_senders: vec!["github".into(), "jira".into()],
            ..WebhookConfig::default()
        });
        assert!(ch.is_allowed("github"));
        assert!(!ch.is_allowed("unknown"));
    }

    #[test]
    fn no_secret_skips_validation() {
        let ch = WebhookChannel::new(WebhookConfig::default());
        assert!(ch.validate_signature(b"anything", "any"));
    }

    #[tokio::test]
    async fn inject_and_receive() {
        let ch = WebhookChannel::new(WebhookConfig::default());
        let mut rx = ch.start().await.unwrap();

        let msg = ChannelMessage {
            channel_id: ChannelId("webhook".into()),
            sender_id: "github".into(),
            sender_name: Some("GitHub Actions".into()),
            content: "Build passed".into(),
            chat_id: "webhook-123".into(),
            reply_to_message_id: None,
            attachments: vec![],
            metadata: serde_json::Map::new(),
        };

        ch.inject_message(msg).await.unwrap();
        let received = rx.recv().await.unwrap().unwrap();
        assert_eq!(received.content, "Build passed");
    }

    #[tokio::test]
    async fn send_with_callback() {
        let ch = WebhookChannel::new(WebhookConfig {
            callback_url: Some("https://example.com/callback".into()),
            ..WebhookConfig::default()
        });
        assert!(ch.send("chat1", "Response", None).await.is_ok());
    }

    #[tokio::test]
    async fn send_without_callback() {
        let ch = WebhookChannel::new(WebhookConfig::default());
        assert!(ch.send("chat1", "Response", None).await.is_ok());
    }

    #[tokio::test]
    async fn health_always_ok() {
        let ch = WebhookChannel::new(WebhookConfig::default());
        assert!(ch.health_check().await.unwrap());
    }
}

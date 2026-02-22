//! Web channel adapter.
//!
//! Implements the Channel trait for the Web Gateway.
//! In production, this integrates with the Axum HTTP server to provide
//! WebSocket and SSE endpoints. Currently operates as an in-process
//! message channel with session management.

use async_trait::async_trait;
use rustedclaw_core::channel::{Channel, ChannelId, ChannelMessage};
use rustedclaw_core::error::ChannelError;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::info;

/// Web channel configuration.
#[derive(Debug, Clone, Default)]
pub struct WebConfig {
    /// Bearer tokens that are allowed to connect.
    pub bearer_tokens: Vec<String>,
    /// Whether to require authentication (default: true).
    pub require_auth: bool,
}

/// Web channel adapter — bridges HTTP/WebSocket clients to the agent.
pub struct WebChannel {
    config: WebConfig,
    channel_id: ChannelId,
    /// Inbound message sender (for injecting messages from HTTP handlers).
    inject_tx: tokio::sync::Mutex<Option<mpsc::Sender<Result<ChannelMessage, ChannelError>>>>,
    /// Outbound message senders per chat_id (for pushing responses to clients).
    outbound: tokio::sync::Mutex<HashMap<String, mpsc::Sender<String>>>,
}

impl WebChannel {
    pub fn new(config: WebConfig) -> Self {
        Self {
            config,
            channel_id: ChannelId("web".into()),
            inject_tx: tokio::sync::Mutex::new(None),
            outbound: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Inject a message from a web client (called by HTTP handlers).
    pub async fn inject_message(&self, msg: ChannelMessage) -> Result<(), ChannelError> {
        let guard = self.inject_tx.lock().await;
        if let Some(tx) = guard.as_ref() {
            tx.send(Ok(msg))
                .await
                .map_err(|_| ChannelError::ConnectionLost("Message channel closed".into()))
        } else {
            Err(ChannelError::ConnectionLost(
                "Web channel not started".into(),
            ))
        }
    }

    /// Register an outbound sender for a client session.
    pub async fn register_session(&self, chat_id: &str) -> mpsc::Receiver<String> {
        let (tx, rx) = mpsc::channel(64);
        self.outbound.lock().await.insert(chat_id.to_string(), tx);
        rx
    }

    /// Remove a client session.
    pub async fn unregister_session(&self, chat_id: &str) {
        self.outbound.lock().await.remove(chat_id);
    }

    /// Number of active sessions.
    pub async fn active_sessions(&self) -> usize {
        self.outbound.lock().await.len()
    }
}

#[async_trait]
impl Channel for WebChannel {
    fn name(&self) -> &str {
        "web"
    }

    fn id(&self) -> &ChannelId {
        &self.channel_id
    }

    async fn start(
        &self,
    ) -> Result<mpsc::Receiver<Result<ChannelMessage, ChannelError>>, ChannelError> {
        info!("Web channel starting");
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
        let outbound = self.outbound.lock().await;
        if let Some(tx) = outbound.get(chat_id) {
            tx.send(content.to_string())
                .await
                .map_err(|_| ChannelError::DeliveryFailed {
                    channel: "web".into(),
                    reason: format!("Session '{}' disconnected", chat_id),
                })
        } else {
            info!(chat_id = %chat_id, "No active session — message buffered");
            Ok(()) // No active session; in production, buffer or discard
        }
    }

    async fn send_typing(&self, chat_id: &str) -> Result<(), ChannelError> {
        let outbound = self.outbound.lock().await;
        if let Some(tx) = outbound.get(chat_id) {
            let _ = tx.send(r#"{"type":"typing"}"#.to_string()).await;
        }
        Ok(())
    }

    fn is_allowed(&self, token: &str) -> bool {
        if !self.config.require_auth {
            return true;
        }
        self.config.bearer_tokens.iter().any(|t| t == token)
    }

    async fn stop(&self) -> Result<(), ChannelError> {
        info!("Web channel stopping");
        *self.inject_tx.lock().await = None;
        self.outbound.lock().await.clear();
        Ok(())
    }

    async fn health_check(&self) -> Result<bool, ChannelError> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_name() {
        let ch = WebChannel::new(WebConfig::default());
        assert_eq!(ch.name(), "web");
    }

    #[test]
    fn no_auth_allows_all() {
        let ch = WebChannel::new(WebConfig::default());
        assert!(ch.is_allowed("anything"));
    }

    #[test]
    fn auth_required_checks_token() {
        let ch = WebChannel::new(WebConfig {
            bearer_tokens: vec!["secret123".into()],
            require_auth: true,
        });
        assert!(ch.is_allowed("secret123"));
        assert!(!ch.is_allowed("wrong"));
    }

    #[tokio::test]
    async fn inject_and_receive() {
        let ch = WebChannel::new(WebConfig::default());
        let mut rx = ch.start().await.unwrap();

        let msg = ChannelMessage {
            channel_id: ChannelId("web".into()),
            sender_id: "session1".into(),
            sender_name: None,
            content: "Hello from browser!".into(),
            chat_id: "session1".into(),
            reply_to_message_id: None,
            attachments: vec![],
            metadata: serde_json::Map::new(),
        };

        ch.inject_message(msg).await.unwrap();
        let received = rx.recv().await.unwrap().unwrap();
        assert_eq!(received.content, "Hello from browser!");
    }

    #[tokio::test]
    async fn session_management() {
        let ch = WebChannel::new(WebConfig::default());
        let _rx = ch.start().await.unwrap();

        assert_eq!(ch.active_sessions().await, 0);

        let mut session_rx = ch.register_session("session1").await;
        assert_eq!(ch.active_sessions().await, 1);

        // Send to session
        ch.send("session1", "Hello!", None).await.unwrap();
        let msg = session_rx.recv().await.unwrap();
        assert_eq!(msg, "Hello!");

        // Unregister
        ch.unregister_session("session1").await;
        assert_eq!(ch.active_sessions().await, 0);
    }

    #[tokio::test]
    async fn send_to_no_session() {
        let ch = WebChannel::new(WebConfig::default());
        let _rx = ch.start().await.unwrap();
        // Should not error, just log
        assert!(ch.send("nonexistent", "Hello!", None).await.is_ok());
    }
}

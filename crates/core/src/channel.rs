//! Channel trait — the abstraction over chat platforms.
//!
//! A Channel connects RustedClaw to a messaging platform (Telegram, Discord,
//! Slack, CLI, webhook, etc.). It receives messages from users and sends
//! responses back.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use crate::error::ChannelError;

/// Unique identifier for a channel instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelId(pub String);

impl std::fmt::Display for ChannelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A message received from or sent to a channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMessage {
    /// The channel this message belongs to
    pub channel_id: ChannelId,

    /// Sender identifier (platform-specific user ID)
    pub sender_id: String,

    /// Human-readable sender name (if available)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_name: Option<String>,

    /// The text content
    pub content: String,

    /// The chat/group/DM identifier within the channel
    pub chat_id: String,

    /// Whether this is a reply to a specific message
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to_message_id: Option<String>,

    /// Attachments (images, files, voice, etc.)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,

    /// Platform-specific metadata
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

/// An attachment in a channel message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    /// Type of attachment
    pub kind: AttachmentKind,

    /// URL or file path
    pub url: String,

    /// Optional filename
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,

    /// MIME type
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,

    /// File size in bytes
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    Image,
    Document,
    Audio,
    Video,
    Voice,
    Other,
}

/// The core Channel trait.
///
/// Implementations handle platform-specific connection logic, message formatting,
/// rate limiting, and authentication.
#[async_trait]
pub trait Channel: Send + Sync {
    /// Human-readable channel name (e.g., "telegram", "discord", "cli").
    fn name(&self) -> &str;

    /// Unique ID for this channel instance.
    fn id(&self) -> &ChannelId;

    /// Start listening for incoming messages.
    ///
    /// Returns a receiver that yields incoming messages. The channel
    /// implementation handles polling, webhooks, or websocket connections
    /// internally.
    async fn start(
        &self,
    ) -> std::result::Result<
        tokio::sync::mpsc::Receiver<std::result::Result<ChannelMessage, ChannelError>>,
        ChannelError,
    >;

    /// Send a response message to a specific chat.
    async fn send(
        &self,
        chat_id: &str,
        content: &str,
        reply_to: Option<&str>,
    ) -> std::result::Result<(), ChannelError>;

    /// Send a typing indicator (if the platform supports it).
    async fn send_typing(&self, _chat_id: &str) -> std::result::Result<(), ChannelError> {
        Ok(()) // No-op default
    }

    /// Check if a sender is allowed (allowlist check).
    fn is_allowed(&self, sender_id: &str) -> bool;

    /// Stop the channel gracefully.
    async fn stop(&self) -> std::result::Result<(), ChannelError> {
        Ok(())
    }

    /// Health check — is the channel connected and operational?
    async fn health_check(&self) -> std::result::Result<bool, ChannelError> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_message_creation() {
        let msg = ChannelMessage {
            channel_id: ChannelId("telegram".into()),
            sender_id: "12345".into(),
            sender_name: Some("Alice".into()),
            content: "Hello bot!".into(),
            chat_id: "67890".into(),
            reply_to_message_id: None,
            attachments: vec![],
            metadata: serde_json::Map::new(),
        };
        assert_eq!(msg.channel_id.0, "telegram");
        assert_eq!(msg.content, "Hello bot!");
    }

    #[test]
    fn attachment_serialization() {
        let attachment = Attachment {
            kind: AttachmentKind::Image,
            url: "https://example.com/photo.jpg".into(),
            filename: Some("photo.jpg".into()),
            mime_type: Some("image/jpeg".into()),
            size_bytes: Some(102400),
        };
        let json = serde_json::to_string(&attachment).unwrap();
        assert!(json.contains("image"));
    }
}

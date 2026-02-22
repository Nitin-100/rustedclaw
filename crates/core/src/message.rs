//! Message and Conversation domain types.
//!
//! These are the core value objects that flow through the entire system:
//! User sends a message → Channel receives it → Agent processes it → Provider generates response.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for a conversation (session).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConversationId(pub String);

impl ConversationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl Default for ConversationId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ConversationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The role of a message sender in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The end user
    User,
    /// The AI assistant
    Assistant,
    /// System instructions (identity, rules)
    System,
    /// Tool execution result
    Tool,
}

/// A single message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Unique message ID
    pub id: String,

    /// Who sent this message
    pub role: Role,

    /// The text content
    pub content: String,

    /// Tool calls requested by the assistant (if any)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<MessageToolCall>,

    /// If this is a tool result, which tool call it responds to
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    /// Timestamp
    pub timestamp: DateTime<Utc>,

    /// Optional metadata (channel info, provider info, etc.)
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

impl Message {
    /// Create a new user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: Role::User,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            timestamp: Utc::now(),
            metadata: serde_json::Map::new(),
        }
    }

    /// Create a new assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            timestamp: Utc::now(),
            metadata: serde_json::Map::new(),
        }
    }

    /// Create a new system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: Role::System,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            timestamp: Utc::now(),
            metadata: serde_json::Map::new(),
        }
    }

    /// Create a tool result message.
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: Role::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
            timestamp: Utc::now(),
            metadata: serde_json::Map::new(),
        }
    }
}

/// A tool call embedded in an assistant message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageToolCall {
    /// Unique ID for this tool call
    pub id: String,

    /// Name of the tool to invoke
    pub name: String,

    /// Arguments as JSON string
    pub arguments: String,
}

/// A conversation is an ordered sequence of messages with shared context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    /// Unique conversation ID
    pub id: ConversationId,

    /// Ordered messages
    pub messages: Vec<Message>,

    /// When this conversation was created
    pub created_at: DateTime<Utc>,

    /// When the last message was added
    pub updated_at: DateTime<Utc>,

    /// Optional title (auto-generated or user-set)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Conversation-level metadata
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

impl Conversation {
    /// Create a new empty conversation.
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            id: ConversationId::new(),
            messages: Vec::new(),
            created_at: now,
            updated_at: now,
            title: None,
            metadata: serde_json::Map::new(),
        }
    }

    /// Add a message to the conversation.
    pub fn push(&mut self, message: Message) {
        self.updated_at = Utc::now();
        self.messages.push(message);
    }

    /// Get the total token count estimate (rough: 4 chars ≈ 1 token).
    pub fn estimated_tokens(&self) -> usize {
        self.messages.iter().map(|m| m.content.len() / 4).sum()
    }
}

impl Default for Conversation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_user_message() {
        let msg = Message::user("Hello, agent!");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "Hello, agent!");
        assert!(msg.tool_calls.is_empty());
    }

    #[test]
    fn conversation_tracks_updates() {
        let mut conv = Conversation::new();
        let created = conv.created_at;

        conv.push(Message::user("First message"));
        assert_eq!(conv.messages.len(), 1);
        assert!(conv.updated_at >= created);
    }

    #[test]
    fn message_serialization_roundtrip() {
        let msg = Message::user("Test message");
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.content, "Test message");
        assert_eq!(deserialized.role, Role::User);
    }

    #[test]
    fn conversation_token_estimate() {
        let mut conv = Conversation::new();
        // 20 chars ≈ 5 tokens
        conv.push(Message::user("12345678901234567890"));
        assert_eq!(conv.estimated_tokens(), 5);
    }
}

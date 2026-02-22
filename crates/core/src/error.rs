//! Error types for the RustedClaw domain.
//!
//! Uses `thiserror` for ergonomic error definitions.
//! Each bounded context has its own error variant.

use thiserror::Error;

/// The top-level error type for all RustedClaw operations.
#[derive(Debug, Error)]
pub enum Error {
    // --- Provider errors ---
    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    // --- Channel errors ---
    #[error("Channel error: {0}")]
    Channel(#[from] ChannelError),

    // --- Memory errors ---
    #[error("Memory error: {0}")]
    Memory(#[from] MemoryError),

    // --- Tool errors ---
    #[error("Tool error: {0}")]
    Tool(#[from] ToolError),

    // --- Configuration errors ---
    #[error("Configuration error: {message}")]
    Config { message: String },

    // --- Serialization ---
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    // --- Generic ---
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result type alias using our Error.
pub type Result<T> = std::result::Result<T, Error>;

// --- Bounded context errors ---

#[derive(Debug, Clone, Error)]
pub enum ProviderError {
    #[error("API request failed: {message} (status: {status_code})")]
    ApiError {
        status_code: u16,
        message: String,
    },

    #[error("Rate limited by provider, retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Stream interrupted: {0}")]
    StreamInterrupted(String),

    #[error("Provider not configured: {0}")]
    NotConfigured(String),

    #[error("Request timed out: {0}")]
    Timeout(String),

    #[error("Network error: {0}")]
    Network(String),
}

#[derive(Debug, Error)]
pub enum ChannelError {
    #[error("Channel not configured: {0}")]
    NotConfigured(String),

    #[error("Message delivery failed to {channel}: {reason}")]
    DeliveryFailed { channel: String, reason: String },

    #[error("Unauthorized sender: {sender_id} on {channel}")]
    Unauthorized { channel: String, sender_id: String },

    #[error("Channel connection lost: {0}")]
    ConnectionLost(String),

    #[error("Invalid webhook payload: {0}")]
    InvalidPayload(String),
}

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Query failed: {0}")]
    QueryFailed(String),

    #[error("Embedding generation failed: {0}")]
    EmbeddingFailed(String),

    #[error("Migration failed: {0}")]
    MigrationFailed(String),
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("Tool not found: {0}")]
    NotFound(String),

    #[error("Tool execution failed: {tool_name} — {reason}")]
    ExecutionFailed { tool_name: String, reason: String },

    #[error("Tool timed out: {tool_name} after {timeout_secs}s")]
    Timeout { tool_name: String, timeout_secs: u64 },

    #[error("Permission denied: {tool_name} — {reason}")]
    PermissionDenied { tool_name: String, reason: String },

    #[error("Sandbox violation: {0}")]
    SandboxViolation(String),

    #[error("Invalid tool arguments: {0}")]
    InvalidArguments(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_error_displays_correctly() {
        let err = Error::Provider(ProviderError::ApiError {
            status_code: 429,
            message: "Too many requests".into(),
        });
        assert!(err.to_string().contains("429"));
        assert!(err.to_string().contains("Too many requests"));
    }

    #[test]
    fn tool_error_displays_correctly() {
        let err = Error::Tool(ToolError::PermissionDenied {
            tool_name: "shell".into(),
            reason: "command not in allowlist".into(),
        });
        assert!(err.to_string().contains("shell"));
        assert!(err.to_string().contains("allowlist"));
    }
}

//! Provider trait — the abstraction over LLM backends.
//!
//! A Provider knows how to send a conversation to an LLM and get a response
//! back, either as a complete message or as a stream of tokens.
//!
//! Implementations: OpenAI-compatible, Anthropic, Ollama, custom endpoints.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use crate::error::ProviderError;
use crate::message::{Message, MessageToolCall};

/// Configuration for a provider request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRequest {
    /// The model to use (e.g., "anthropic/claude-sonnet-4", "gpt-4o")
    pub model: String,

    /// The conversation messages
    pub messages: Vec<Message>,

    /// Temperature (0.0 = deterministic, 1.0 = creative)
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    /// Maximum tokens to generate
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Available tools the model can call
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDefinition>,

    /// Whether to stream the response
    #[serde(default)]
    pub stream: bool,

    /// Stop sequences
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop: Vec<String>,
}

fn default_temperature() -> f32 {
    0.7
}

/// A tool definition sent to the LLM so it knows what tools it can call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// The tool name
    pub name: String,

    /// Description of what the tool does
    pub description: String,

    /// JSON Schema describing the tool's parameters
    pub parameters: serde_json::Value,
}

/// A complete (non-streaming) response from a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResponse {
    /// The generated message
    pub message: Message,

    /// Token usage statistics
    pub usage: Option<Usage>,

    /// Which model actually responded (may differ from requested)
    pub model: String,

    /// Provider-specific metadata
    #[serde(default)]
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

/// Token usage information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// A single chunk in a streaming response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    /// Partial content delta
    #[serde(default)]
    pub content: Option<String>,

    /// Partial tool call deltas
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<MessageToolCall>,

    /// Whether this is the final chunk
    #[serde(default)]
    pub done: bool,

    /// Usage info (typically only in the final chunk)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// An embedding request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingRequest {
    /// The model to use for embeddings (e.g., "text-embedding-3-small").
    pub model: String,

    /// The texts to embed.
    pub inputs: Vec<String>,
}

/// An embedding response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    /// The embedding vectors, one per input text.
    pub embeddings: Vec<Vec<f32>>,

    /// Which model was used.
    pub model: String,

    /// Token usage.
    pub usage: Option<Usage>,
}

/// The core Provider trait.
///
/// Every LLM backend (OpenAI, Anthropic, Ollama, custom) implements this trait.
/// The agent loop calls `complete()` or `stream()` without knowing which provider
/// is being used — pure polymorphism.
#[async_trait]
pub trait Provider: Send + Sync {
    /// A human-readable name for this provider (e.g., "openrouter", "anthropic").
    fn name(&self) -> &str;

    /// Send a request and get a complete response.
    async fn complete(&self, request: ProviderRequest) -> std::result::Result<ProviderResponse, ProviderError>;

    /// Send a request and get a stream of response chunks.
    ///
    /// Default implementation calls `complete()` and wraps the result as a single chunk.
    async fn stream(
        &self,
        request: ProviderRequest,
    ) -> std::result::Result<
        tokio::sync::mpsc::Receiver<std::result::Result<StreamChunk, ProviderError>>,
        ProviderError,
    > {
        let response = self.complete(request).await?;
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let _ = tx.send(Ok(StreamChunk {
            content: Some(response.message.content),
            tool_calls: response.message.tool_calls,
            done: true,
            usage: response.usage,
        })).await;
        Ok(rx)
    }

    /// Generate embeddings for the given texts.
    ///
    /// Default implementation returns an error indicating embeddings aren't supported.
    async fn embed(
        &self,
        _request: EmbeddingRequest,
    ) -> std::result::Result<EmbeddingResponse, ProviderError> {
        Err(ProviderError::NotConfigured(
            format!("Provider '{}' does not support embeddings", self.name()),
        ))
    }

    /// List available models for this provider.
    async fn list_models(&self) -> std::result::Result<Vec<String>, ProviderError> {
        Ok(Vec::new())
    }

    /// Health check — can we reach the provider?
    async fn health_check(&self) -> std::result::Result<bool, ProviderError> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_request_defaults() {
        let req = ProviderRequest {
            model: "gpt-4o".into(),
            messages: vec![],
            temperature: default_temperature(),
            max_tokens: None,
            tools: vec![],
            stream: false,
            stop: vec![],
        };
        assert!((req.temperature - 0.7).abs() < f32::EPSILON);
        assert!(!req.stream);
    }

    #[test]
    fn tool_definition_serialization() {
        let tool = ToolDefinition {
            name: "shell".into(),
            description: "Execute a shell command".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The command to run" }
                },
                "required": ["command"]
            }),
        };
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("shell"));
        assert!(json.contains("command"));
    }
}

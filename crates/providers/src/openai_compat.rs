//! OpenAI-compatible provider implementation.
//!
//! Works with: OpenAI, OpenRouter, Anthropic (via proxy), Ollama, vLLM,
//! Together AI, Fireworks AI, and any OpenAI-compatible endpoint.
//!
//! Supports:
//! - Chat completions (non-streaming and streaming SSE)
//! - Tool use / function calling
//! - Model listing and health checks

use async_trait::async_trait;
use futures::StreamExt;
use rustedclaw_core::error::ProviderError;
use rustedclaw_core::message::{Message, MessageToolCall, Role};
use rustedclaw_core::provider::*;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace, warn};

/// An OpenAI-compatible LLM provider.
///
/// This handles the vast majority of LLM providers since most expose
/// an OpenAI-compatible `/v1/chat/completions` endpoint.
pub struct OpenAiCompatProvider {
    name: String,
    base_url: String,
    api_key: String,
    client: reqwest::Client,
}

impl OpenAiCompatProvider {
    /// Create a new OpenAI-compatible provider.
    pub fn new(
        name: impl Into<String>,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            name: name.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            client,
        }
    }

    /// Create an OpenRouter provider (convenience constructor).
    pub fn openrouter(api_key: impl Into<String>) -> Self {
        Self::new("openrouter", "https://openrouter.ai/api/v1", api_key)
    }

    /// Create an OpenAI provider (convenience constructor).
    pub fn openai(api_key: impl Into<String>) -> Self {
        Self::new("openai", "https://api.openai.com/v1", api_key)
    }

    /// Create an Ollama provider (convenience constructor).
    pub fn ollama(base_url: Option<&str>) -> Self {
        Self::new(
            "ollama",
            base_url.unwrap_or("http://localhost:11434/v1"),
            "ollama", // Ollama doesn't need a real key
        )
    }

    /// Convert our Message types to OpenAI API format.
    fn to_api_messages(messages: &[Message]) -> Vec<ApiMessage> {
        messages
            .iter()
            .map(|m| ApiMessage {
                role: match m.role {
                    Role::User => "user".into(),
                    Role::Assistant => "assistant".into(),
                    Role::System => "system".into(),
                    Role::Tool => "tool".into(),
                },
                content: Some(m.content.clone()),
                tool_calls: if m.tool_calls.is_empty() {
                    None
                } else {
                    Some(
                        m.tool_calls
                            .iter()
                            .map(|tc| ApiToolCall {
                                id: tc.id.clone(),
                                r#type: "function".into(),
                                function: ApiFunction {
                                    name: tc.name.clone(),
                                    arguments: tc.arguments.clone(),
                                },
                            })
                            .collect(),
                    )
                },
                tool_call_id: m.tool_call_id.clone(),
            })
            .collect()
    }

    /// Convert tool definitions to OpenAI API format.
    fn to_api_tools(tools: &[ToolDefinition]) -> Vec<ApiToolDefinition> {
        tools
            .iter()
            .map(|t| ApiToolDefinition {
                r#type: "function".into(),
                function: ApiToolFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                },
            })
            .collect()
    }
}

#[async_trait]
impl rustedclaw_core::Provider for OpenAiCompatProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn complete(
        &self,
        request: ProviderRequest,
    ) -> std::result::Result<ProviderResponse, ProviderError> {
        let url = format!("{}/chat/completions", self.base_url);

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": Self::to_api_messages(&request.messages),
            "temperature": request.temperature,
            "stream": false,
        });

        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }

        if !request.tools.is_empty() {
            body["tools"] = serde_json::json!(Self::to_api_tools(&request.tools));
        }

        if !request.stop.is_empty() {
            body["stop"] = serde_json::json!(request.stop);
        }

        debug!(provider = %self.name, model = %request.model, "Sending completion request");

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        let status = response.status().as_u16();

        if status == 429 {
            return Err(ProviderError::RateLimited {
                retry_after_secs: 5,
            });
        }

        if status == 401 || status == 403 {
            return Err(ProviderError::AuthenticationFailed(
                "Invalid API key or insufficient permissions".into(),
            ));
        }

        if status != 200 {
            let error_body = response.text().await.unwrap_or_default();
            warn!(status, body = %error_body, "Provider returned error");
            return Err(ProviderError::ApiError {
                status_code: status,
                message: error_body,
            });
        }

        let api_response: ApiResponse =
            response.json().await.map_err(|e| ProviderError::ApiError {
                status_code: 200,
                message: format!("Failed to parse response: {e}"),
            })?;

        let choice =
            api_response
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| ProviderError::ApiError {
                    status_code: 200,
                    message: "No choices in response".into(),
                })?;

        let tool_calls: Vec<MessageToolCall> = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|tc| MessageToolCall {
                id: tc.id,
                name: tc.function.name,
                arguments: tc.function.arguments,
            })
            .collect();

        let message = Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: choice.message.content.unwrap_or_default(),
            tool_calls,
            tool_call_id: None,
            timestamp: chrono::Utc::now(),
            metadata: serde_json::Map::new(),
        };

        let usage = api_response.usage.map(|u| Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });

        Ok(ProviderResponse {
            message,
            usage,
            model: api_response.model,
            metadata: serde_json::Map::new(),
        })
    }

    async fn list_models(&self) -> std::result::Result<Vec<String>, ProviderError> {
        let url = format!("{}/models", self.base_url);
        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Ok(Vec::new());
        }

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        let models = body["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["id"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }

    async fn health_check(&self) -> std::result::Result<bool, ProviderError> {
        let url = format!("{}/models", self.base_url);
        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        Ok(response.status().is_success())
    }

    async fn embed(
        &self,
        request: EmbeddingRequest,
    ) -> std::result::Result<EmbeddingResponse, ProviderError> {
        let url = format!("{}/embeddings", self.base_url);

        let body = serde_json::json!({
            "model": request.model,
            "input": request.inputs,
            "encoding_format": "float",
        });

        debug!(
            provider = %self.name,
            model = %request.model,
            count = request.inputs.len(),
            "Sending embedding request"
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        let status = response.status().as_u16();

        if status == 429 {
            return Err(ProviderError::RateLimited {
                retry_after_secs: 5,
            });
        }
        if status == 401 || status == 403 {
            return Err(ProviderError::AuthenticationFailed(
                "Invalid API key".into(),
            ));
        }
        if status != 200 {
            let error_body = response.text().await.unwrap_or_default();
            return Err(ProviderError::ApiError {
                status_code: status,
                message: error_body,
            });
        }

        let api_resp: EmbeddingApiResponse =
            response.json().await.map_err(|e| ProviderError::ApiError {
                status_code: 200,
                message: format!("Failed to parse embedding response: {e}"),
            })?;

        let embeddings = api_resp.data.into_iter().map(|d| d.embedding).collect();

        let usage = api_resp.usage.map(|u| Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: 0,
            total_tokens: u.total_tokens,
        });

        Ok(EmbeddingResponse {
            embeddings,
            model: api_resp.model,
            usage,
        })
    }

    async fn stream(
        &self,
        request: ProviderRequest,
    ) -> std::result::Result<
        tokio::sync::mpsc::Receiver<std::result::Result<StreamChunk, ProviderError>>,
        ProviderError,
    > {
        let url = format!("{}/chat/completions", self.base_url);

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": Self::to_api_messages(&request.messages),
            "temperature": request.temperature,
            "stream": true,
            "stream_options": { "include_usage": true },
        });

        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }

        if !request.tools.is_empty() {
            body["tools"] = serde_json::json!(Self::to_api_tools(&request.tools));
        }

        if !request.stop.is_empty() {
            body["stop"] = serde_json::json!(request.stop);
        }

        debug!(provider = %self.name, model = %request.model, "Sending streaming request");

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        let status = response.status().as_u16();

        if status == 429 {
            return Err(ProviderError::RateLimited {
                retry_after_secs: 5,
            });
        }

        if status == 401 || status == 403 {
            return Err(ProviderError::AuthenticationFailed(
                "Invalid API key or insufficient permissions".into(),
            ));
        }

        if status != 200 {
            let error_body = response.text().await.unwrap_or_default();
            warn!(status, body = %error_body, "Provider streaming error");
            return Err(ProviderError::ApiError {
                status_code: status,
                message: error_body,
            });
        }

        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let provider_name = self.name.clone();

        // Spawn task to read the SSE byte stream and parse chunks
        tokio::spawn(async move {
            let mut byte_stream = response.bytes_stream();
            let mut buffer = String::new();

            // Accumulators for tool call deltas (keyed by index)
            let mut tool_call_accumulators: std::collections::HashMap<u32, ToolCallAccumulator> =
                std::collections::HashMap::new();

            while let Some(chunk_result) = byte_stream.next().await {
                let bytes = match chunk_result {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx
                            .send(Err(ProviderError::StreamInterrupted(e.to_string())))
                            .await;
                        return;
                    }
                };

                // Append new bytes to our line buffer
                buffer.push_str(&String::from_utf8_lossy(&bytes));

                // Process complete lines
                while let Some(line_end) = buffer.find('\n') {
                    let line = buffer[..line_end].trim_end_matches('\r').to_string();
                    buffer = buffer[line_end + 1..].to_string();

                    // Skip empty lines and SSE comments
                    if line.is_empty() || line.starts_with(':') {
                        continue;
                    }

                    // Handle "data: ..." lines
                    if let Some(data) = line.strip_prefix("data: ") {
                        let data = data.trim();

                        // "[DONE]" signals end of stream
                        if data == "[DONE]" {
                            // Emit final done chunk with accumulated tool calls
                            let final_tool_calls: Vec<MessageToolCall> = tool_call_accumulators
                                .values()
                                .map(|acc| acc.to_tool_call())
                                .collect();

                            let _ = tx
                                .send(Ok(StreamChunk {
                                    content: None,
                                    tool_calls: final_tool_calls,
                                    done: true,
                                    usage: None,
                                }))
                                .await;
                            return;
                        }

                        // Parse the JSON chunk
                        match serde_json::from_str::<StreamResponse>(data) {
                            Ok(stream_resp) => {
                                if let Some(choice) = stream_resp.choices.first() {
                                    let delta = &choice.delta;

                                    // Accumulate tool call deltas
                                    if let Some(ref tc_deltas) = delta.tool_calls {
                                        for tc_delta in tc_deltas {
                                            let acc = tool_call_accumulators
                                                .entry(tc_delta.index)
                                                .or_insert_with(|| ToolCallAccumulator {
                                                    id: String::new(),
                                                    name: String::new(),
                                                    arguments: String::new(),
                                                });

                                            if let Some(ref id) = tc_delta.id {
                                                acc.id = id.clone();
                                            }
                                            if let Some(ref func) = tc_delta.function {
                                                if let Some(ref name) = func.name {
                                                    acc.name = name.clone();
                                                }
                                                if let Some(ref args) = func.arguments {
                                                    acc.arguments.push_str(args);
                                                }
                                            }
                                        }
                                    }

                                    // Send content delta
                                    let has_content =
                                        delta.content.as_ref().is_some_and(|c| !c.is_empty());
                                    let is_finish = choice.finish_reason.is_some();

                                    if has_content || is_finish {
                                        let chunk = StreamChunk {
                                            content: delta.content.clone(),
                                            tool_calls: Vec::new(),
                                            done: false,
                                            usage: None,
                                        };

                                        if tx.send(Ok(chunk)).await.is_err() {
                                            return; // receiver dropped
                                        }
                                    }
                                }

                                // Handle usage in final stream chunk (stream_options)
                                if let Some(usage) = stream_resp.usage {
                                    let final_tool_calls: Vec<MessageToolCall> =
                                        tool_call_accumulators
                                            .values()
                                            .map(|acc| acc.to_tool_call())
                                            .collect();

                                    let chunk = StreamChunk {
                                        content: None,
                                        tool_calls: final_tool_calls,
                                        done: true,
                                        usage: Some(Usage {
                                            prompt_tokens: usage.prompt_tokens,
                                            completion_tokens: usage.completion_tokens,
                                            total_tokens: usage.total_tokens,
                                        }),
                                    };

                                    let _ = tx.send(Ok(chunk)).await;
                                    return;
                                }
                            }
                            Err(e) => {
                                trace!(
                                    provider = %provider_name,
                                    data = %data,
                                    error = %e,
                                    "Ignoring unparseable SSE chunk"
                                );
                            }
                        }
                    }
                }
            }

            // Stream ended without [DONE] — send final chunk
            let final_tool_calls: Vec<MessageToolCall> = tool_call_accumulators
                .values()
                .map(|acc| acc.to_tool_call())
                .collect();

            let _ = tx
                .send(Ok(StreamChunk {
                    content: None,
                    tool_calls: final_tool_calls,
                    done: true,
                    usage: None,
                }))
                .await;
        });

        Ok(rx)
    }
}

// --- OpenAI API types (internal) ---

#[derive(Debug, Serialize, Deserialize)]
struct ApiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ApiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiToolCall {
    id: String,
    r#type: String,
    function: ApiFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiToolDefinition {
    r#type: String,
    function: ApiToolFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiToolFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    model: String,
    choices: Vec<ApiChoice>,
    usage: Option<ApiUsage>,
}

#[derive(Debug, Deserialize)]
struct ApiChoice {
    message: ApiMessage,
}

#[derive(Debug, Deserialize)]
struct ApiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

// --- Embedding API types ---

#[derive(Debug, Deserialize)]
struct EmbeddingApiResponse {
    data: Vec<EmbeddingData>,
    model: String,
    usage: Option<EmbeddingApiUsage>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingApiUsage {
    prompt_tokens: u32,
    total_tokens: u32,
}

// --- Streaming SSE types ---

/// A single SSE `data: {...}` chunk from a streaming response.
#[derive(Debug, Deserialize)]
struct StreamResponse {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<ApiUsage>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<StreamToolCallDelta>>,
}

/// A tool call delta — arrives incrementally across chunks.
#[derive(Debug, Deserialize)]
struct StreamToolCallDelta {
    index: u32,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<StreamFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct StreamFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// Accumulates incremental tool call deltas into a complete tool call.
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}

impl ToolCallAccumulator {
    fn to_tool_call(&self) -> MessageToolCall {
        MessageToolCall {
            id: self.id.clone(),
            name: self.name.clone(),
            arguments: self.arguments.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openrouter_constructor() {
        let provider = OpenAiCompatProvider::openrouter("sk-test");
        assert_eq!(provider.name(), "openrouter");
        assert!(provider.base_url.contains("openrouter.ai"));
    }

    #[test]
    fn ollama_constructor() {
        let provider = OpenAiCompatProvider::ollama(None);
        assert_eq!(provider.name(), "ollama");
        assert!(provider.base_url.contains("localhost:11434"));
    }

    #[test]
    fn message_conversion() {
        let messages = vec![Message::system("You are helpful"), Message::user("Hello")];
        let api_messages = OpenAiCompatProvider::to_api_messages(&messages);
        assert_eq!(api_messages.len(), 2);
        assert_eq!(api_messages[0].role, "system");
        assert_eq!(api_messages[1].role, "user");
    }

    #[test]
    fn tool_definition_conversion() {
        let tools = vec![ToolDefinition {
            name: "shell".into(),
            description: "Run a shell command".into(),
            parameters: serde_json::json!({"type": "object"}),
        }];
        let api_tools = OpenAiCompatProvider::to_api_tools(&tools);
        assert_eq!(api_tools.len(), 1);
        assert_eq!(api_tools[0].function.name, "shell");
        assert_eq!(api_tools[0].r#type, "function");
    }

    // --- SSE parsing tests ---

    #[test]
    fn parse_stream_content_delta() {
        let data = r#"{"choices":[{"delta":{"content":"Hello"},"finish_reason":null}]}"#;
        let parsed: StreamResponse = serde_json::from_str(data).unwrap();
        assert_eq!(parsed.choices.len(), 1);
        assert_eq!(parsed.choices[0].delta.content.as_deref(), Some("Hello"));
        assert!(parsed.choices[0].finish_reason.is_none());
    }

    #[test]
    fn parse_stream_finish_chunk() {
        let data = r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#;
        let parsed: StreamResponse = serde_json::from_str(data).unwrap();
        assert_eq!(parsed.choices[0].finish_reason.as_deref(), Some("stop"));
        assert!(parsed.choices[0].delta.content.is_none());
    }

    #[test]
    fn parse_stream_tool_call_delta() {
        let data = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_abc","function":{"name":"calculator","arguments":""}}]},"finish_reason":null}]}"#;
        let parsed: StreamResponse = serde_json::from_str(data).unwrap();
        let tc = &parsed.choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.index, 0);
        assert_eq!(tc.id.as_deref(), Some("call_abc"));
        assert_eq!(
            tc.function.as_ref().unwrap().name.as_deref(),
            Some("calculator")
        );
    }

    #[test]
    fn parse_stream_tool_call_arguments_delta() {
        // Arguments arrive incrementally as fragments
        let data = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"expr\""}}]},"finish_reason":null}]}"#;
        let parsed: StreamResponse = serde_json::from_str(data).unwrap();
        let tc = &parsed.choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert!(tc.id.is_none()); // ID only in first delta
        assert_eq!(
            tc.function.as_ref().unwrap().arguments.as_deref(),
            Some("{\"expr\"")
        );
    }

    #[test]
    fn parse_stream_usage() {
        let data = r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#;
        let parsed: StreamResponse = serde_json::from_str(data).unwrap();
        let usage = parsed.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
    }

    #[test]
    fn tool_call_accumulator_assembly() {
        let mut acc = ToolCallAccumulator {
            id: String::new(),
            name: String::new(),
            arguments: String::new(),
        };

        // First delta: id + name
        acc.id = "call_123".into();
        acc.name = "calculator".into();
        acc.arguments.push_str("{\"expr\"");

        // Second delta: more arguments
        acc.arguments.push_str(": \"2+2\"}");

        let tc = acc.to_tool_call();
        assert_eq!(tc.id, "call_123");
        assert_eq!(tc.name, "calculator");
        assert_eq!(tc.arguments, "{\"expr\": \"2+2\"}");
    }

    #[test]
    fn parse_empty_delta() {
        let data = r#"{"choices":[{"delta":{},"finish_reason":null}]}"#;
        let parsed: StreamResponse = serde_json::from_str(data).unwrap();
        assert!(parsed.choices[0].delta.content.is_none());
        assert!(parsed.choices[0].delta.tool_calls.is_none());
    }

    #[test]
    fn message_conversion_with_tool_calls() {
        let mut msg = Message::assistant("thinking...");
        msg.tool_calls = vec![MessageToolCall {
            id: "call_1".into(),
            name: "shell".into(),
            arguments: r#"{"command":"ls"}"#.into(),
        }];
        let api_msgs = OpenAiCompatProvider::to_api_messages(&[msg]);
        assert_eq!(api_msgs.len(), 1);
        let tc = api_msgs[0].tool_calls.as_ref().unwrap();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].function.name, "shell");
    }

    #[test]
    fn message_conversion_tool_response() {
        let msg = Message::tool_result("call_1", "result data");
        let api_msgs = OpenAiCompatProvider::to_api_messages(&[msg]);
        assert_eq!(api_msgs[0].role, "tool");
        assert_eq!(api_msgs[0].tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn parse_multiple_tool_calls_in_stream() {
        // Two parallel tool calls in one delta
        let data = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"name":"search","arguments":""}},{"index":1,"id":"call_b","function":{"name":"calc","arguments":""}}]},"finish_reason":null}]}"#;
        let parsed: StreamResponse = serde_json::from_str(data).unwrap();
        let tcs = parsed.choices[0].delta.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 2);
        assert_eq!(tcs[0].index, 0);
        assert_eq!(tcs[1].index, 1);
    }

    #[test]
    fn parse_embedding_response() {
        let data = r#"{
            "data": [
                {"embedding": [0.1, 0.2, 0.3], "index": 0},
                {"embedding": [0.4, 0.5, 0.6], "index": 1}
            ],
            "model": "text-embedding-3-small",
            "usage": {"prompt_tokens": 8, "total_tokens": 8}
        }"#;
        let parsed: EmbeddingApiResponse = serde_json::from_str(data).unwrap();
        assert_eq!(parsed.data.len(), 2);
        assert_eq!(parsed.data[0].embedding, vec![0.1, 0.2, 0.3]);
        assert_eq!(parsed.data[1].embedding, vec![0.4, 0.5, 0.6]);
        assert_eq!(parsed.model, "text-embedding-3-small");
        assert_eq!(parsed.usage.unwrap().prompt_tokens, 8);
    }

    #[test]
    fn embedding_request_types() {
        let req = EmbeddingRequest {
            model: "text-embedding-3-small".into(),
            inputs: vec!["hello world".into(), "how are you".into()],
        };
        assert_eq!(req.inputs.len(), 2);
        assert_eq!(req.model, "text-embedding-3-small");
    }
}

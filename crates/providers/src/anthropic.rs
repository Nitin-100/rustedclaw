//! Anthropic native provider implementation.
//!
//! Uses Anthropic's Messages API directly (not OpenAI-compatible proxy).
//!
//! Features:
//! - `x-api-key` header authentication (not Bearer)
//! - `anthropic-version` header
//! - System prompt as top-level field
//! - Native tool use with `tool_use` / `tool_result` content blocks
//! - Streaming via SSE with `content_block_delta` events
//! - Extended thinking support

use async_trait::async_trait;
use futures::StreamExt;
use rustedclaw_core::error::ProviderError;
use rustedclaw_core::message::{Message, MessageToolCall, Role};
use rustedclaw_core::provider::*;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace, warn};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Anthropic native Messages API provider.
pub struct AnthropicProvider {
    name: String,
    base_url: String,
    api_key: String,
    client: reqwest::Client,
    /// Enable extended thinking (beta feature).
    extended_thinking: bool,
    /// Budget tokens for extended thinking.
    thinking_budget: Option<u32>,
}

impl AnthropicProvider {
    /// Create a new Anthropic provider.
    pub fn new(api_key: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300)) // Anthropic can be slow with thinking
            .build()
            .expect("Failed to create HTTP client");

        Self {
            name: "anthropic".into(),
            base_url: DEFAULT_BASE_URL.into(),
            api_key: api_key.into(),
            client,
            extended_thinking: false,
            thinking_budget: None,
        }
    }

    /// Create with a custom base URL (e.g., for testing or proxies).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }

    /// Enable extended thinking.
    pub fn with_extended_thinking(mut self, budget_tokens: u32) -> Self {
        self.extended_thinking = true;
        self.thinking_budget = Some(budget_tokens);
        self
    }

    /// Extract system messages from the message list.
    /// Anthropic puts system prompt as a top-level field, not in messages.
    fn extract_system(messages: &[Message]) -> (Option<String>, Vec<&Message>) {
        let mut system_parts: Vec<&str> = Vec::new();
        let mut non_system: Vec<&Message> = Vec::new();

        for msg in messages {
            match msg.role {
                Role::System => system_parts.push(&msg.content),
                _ => non_system.push(msg),
            }
        }

        let system = if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n\n"))
        };

        (system, non_system)
    }

    /// Convert messages to Anthropic API format with content blocks.
    fn to_api_messages(messages: &[&Message]) -> Vec<AnthropicMessage> {
        let mut result = Vec::new();

        for msg in messages {
            match msg.role {
                Role::User => {
                    result.push(AnthropicMessage {
                        role: "user".into(),
                        content: AnthropicContent::Text(msg.content.clone()),
                    });
                }
                Role::Assistant => {
                    if msg.tool_calls.is_empty() {
                        result.push(AnthropicMessage {
                            role: "assistant".into(),
                            content: AnthropicContent::Text(msg.content.clone()),
                        });
                    } else {
                        // Assistant message with tool use blocks
                        let mut blocks: Vec<ContentBlock> = Vec::new();
                        if !msg.content.is_empty() {
                            blocks.push(ContentBlock::Text {
                                text: msg.content.clone(),
                            });
                        }
                        for tc in &msg.tool_calls {
                            let input: serde_json::Value =
                                serde_json::from_str(&tc.arguments).unwrap_or_default();
                            blocks.push(ContentBlock::ToolUse {
                                id: tc.id.clone(),
                                name: tc.name.clone(),
                                input,
                            });
                        }
                        result.push(AnthropicMessage {
                            role: "assistant".into(),
                            content: AnthropicContent::Blocks(blocks),
                        });
                    }
                }
                Role::Tool => {
                    // Tool results
                    let tool_call_id =
                        msg.tool_call_id.clone().unwrap_or_default();
                    result.push(AnthropicMessage {
                        role: "user".into(),
                        content: AnthropicContent::Blocks(vec![
                            ContentBlock::ToolResult {
                                tool_use_id: tool_call_id,
                                content: msg.content.clone(),
                            },
                        ]),
                    });
                }
                Role::System => {} // handled separately
            }
        }

        result
    }

    /// Convert tool definitions to Anthropic format.
    fn to_api_tools(tools: &[ToolDefinition]) -> Vec<AnthropicTool> {
        tools
            .iter()
            .map(|t| AnthropicTool {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.parameters.clone(),
            })
            .collect()
    }
}

#[async_trait]
impl rustedclaw_core::Provider for AnthropicProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn complete(
        &self,
        request: ProviderRequest,
    ) -> std::result::Result<ProviderResponse, ProviderError> {
        let url = format!("{}/v1/messages", self.base_url);
        let (system, messages) = Self::extract_system(&request.messages);
        let api_messages = Self::to_api_messages(&messages);

        let max_tokens = request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": api_messages,
            "max_tokens": max_tokens,
            "temperature": request.temperature,
        });

        if let Some(ref sys) = system {
            body["system"] = serde_json::json!(sys);
        }

        if !request.tools.is_empty() {
            body["tools"] = serde_json::json!(Self::to_api_tools(&request.tools));
        }

        if !request.stop.is_empty() {
            body["stop_sequences"] = serde_json::json!(request.stop);
        }

        if self.extended_thinking {
            if let Some(budget) = self.thinking_budget {
                body["thinking"] = serde_json::json!({
                    "type": "enabled",
                    "budget_tokens": budget
                });
            }
        }

        debug!(provider = "anthropic", model = %request.model, "Sending completion request");

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
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
                "Invalid Anthropic API key".into(),
            ));
        }
        if status != 200 {
            let error_body = response.text().await.unwrap_or_default();
            warn!(status, body = %error_body, "Anthropic API error");
            return Err(ProviderError::ApiError {
                status_code: status,
                message: error_body,
            });
        }

        let api_resp: AnthropicResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::ApiError {
                status_code: 200,
                message: format!("Failed to parse Anthropic response: {e}"),
            })?;

        Self::response_to_provider_response(api_resp)
    }

    async fn stream(
        &self,
        request: ProviderRequest,
    ) -> std::result::Result<
        tokio::sync::mpsc::Receiver<std::result::Result<StreamChunk, ProviderError>>,
        ProviderError,
    > {
        let url = format!("{}/v1/messages", self.base_url);
        let (system, messages) = Self::extract_system(&request.messages);
        let api_messages = Self::to_api_messages(&messages);

        let max_tokens = request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": api_messages,
            "max_tokens": max_tokens,
            "temperature": request.temperature,
            "stream": true,
        });

        if let Some(ref sys) = system {
            body["system"] = serde_json::json!(sys);
        }

        if !request.tools.is_empty() {
            body["tools"] = serde_json::json!(Self::to_api_tools(&request.tools));
        }

        if !request.stop.is_empty() {
            body["stop_sequences"] = serde_json::json!(request.stop);
        }

        if self.extended_thinking {
            if let Some(budget) = self.thinking_budget {
                body["thinking"] = serde_json::json!({
                    "type": "enabled",
                    "budget_tokens": budget
                });
            }
        }

        debug!(provider = "anthropic", model = %request.model, "Sending streaming request");

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
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
                "Invalid Anthropic API key".into(),
            ));
        }
        if status != 200 {
            let error_body = response.text().await.unwrap_or_default();
            return Err(ProviderError::ApiError {
                status_code: status,
                message: error_body,
            });
        }

        let (tx, rx) = tokio::sync::mpsc::channel(64);

        tokio::spawn(async move {
            let mut byte_stream = response.bytes_stream();
            let mut buffer = String::new();

            // Accumulators
            let mut current_tool_id = String::new();
            let mut current_tool_name = String::new();
            let mut tool_args_buffer = String::new();
            let mut tool_calls: Vec<MessageToolCall> = Vec::new();
            let mut in_tool_use = false;

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

                buffer.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(line_end) = buffer.find('\n') {
                    let line = buffer[..line_end].trim_end_matches('\r').to_string();
                    buffer = buffer[line_end + 1..].to_string();

                    if line.is_empty() || line.starts_with(':') {
                        continue;
                    }

                    if let Some(event_type) = line.strip_prefix("event: ") {
                        // Anthropic uses typed events
                        if event_type.trim() == "message_stop" {
                            // Finalize any pending tool call
                            if in_tool_use {
                                tool_calls.push(MessageToolCall {
                                    id: std::mem::take(&mut current_tool_id),
                                    name: std::mem::take(&mut current_tool_name),
                                    arguments: std::mem::take(&mut tool_args_buffer),
                                });
                                in_tool_use = false;
                            }

                            let _ = tx
                                .send(Ok(StreamChunk {
                                    content: None,
                                    tool_calls: std::mem::take(&mut tool_calls),
                                    done: true,
                                    usage: None,
                                }))
                                .await;
                            return;
                        }
                        continue;
                    }

                    if let Some(data) = line.strip_prefix("data: ") {
                        let data = data.trim();
                        if data.is_empty() {
                            continue;
                        }

                        let event: serde_json::Value = match serde_json::from_str(data) {
                            Ok(v) => v,
                            Err(e) => {
                                trace!(error = %e, data = %data, "Ignoring unparseable Anthropic SSE");
                                continue;
                            }
                        };

                        let event_type = event["type"].as_str().unwrap_or("");

                        match event_type {
                            "content_block_start" => {
                                let block = &event["content_block"];
                                if block["type"].as_str() == Some("tool_use") {
                                    // Finalize previous tool if any
                                    if in_tool_use {
                                        tool_calls.push(MessageToolCall {
                                            id: std::mem::take(&mut current_tool_id),
                                            name: std::mem::take(&mut current_tool_name),
                                            arguments: std::mem::take(&mut tool_args_buffer),
                                        });
                                    }
                                    current_tool_id =
                                        block["id"].as_str().unwrap_or("").to_string();
                                    current_tool_name =
                                        block["name"].as_str().unwrap_or("").to_string();
                                    tool_args_buffer.clear();
                                    in_tool_use = true;
                                }
                            }
                            "content_block_delta" => {
                                let delta = &event["delta"];
                                let delta_type = delta["type"].as_str().unwrap_or("");

                                match delta_type {
                                    "text_delta" => {
                                        if let Some(text) = delta["text"].as_str() {
                                            let chunk = StreamChunk {
                                                content: Some(text.to_string()),
                                                tool_calls: Vec::new(),
                                                done: false,
                                                usage: None,
                                            };
                                            if tx.send(Ok(chunk)).await.is_err() {
                                                return;
                                            }
                                        }
                                    }
                                    "input_json_delta" => {
                                        if let Some(partial) = delta["partial_json"].as_str() {
                                            tool_args_buffer.push_str(partial);
                                        }
                                    }
                                    "thinking_delta" => {
                                        // Extended thinking — emit as content with metadata marker
                                        if let Some(thinking) = delta["thinking"].as_str() {
                                            let chunk = StreamChunk {
                                                content: Some(thinking.to_string()),
                                                tool_calls: Vec::new(),
                                                done: false,
                                                usage: None,
                                            };
                                            if tx.send(Ok(chunk)).await.is_err() {
                                                return;
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            "content_block_stop" => {
                                if in_tool_use {
                                    tool_calls.push(MessageToolCall {
                                        id: std::mem::take(&mut current_tool_id),
                                        name: std::mem::take(&mut current_tool_name),
                                        arguments: std::mem::take(&mut tool_args_buffer),
                                    });
                                    in_tool_use = false;
                                }
                            }
                            "message_delta" => {
                                // May contain usage
                                if let Some(usage) = event.get("usage") {
                                    if let (Some(out), Some(inp)) = (
                                        usage["output_tokens"].as_u64(),
                                        usage.get("input_tokens").and_then(|v| v.as_u64()),
                                    ) {
                                        let u = Usage {
                                            prompt_tokens: inp as u32,
                                            completion_tokens: out as u32,
                                            total_tokens: (inp + out) as u32,
                                        };
                                        let _ = tx
                                            .send(Ok(StreamChunk {
                                                content: None,
                                                tool_calls: Vec::new(),
                                                done: false,
                                                usage: Some(u),
                                            }))
                                            .await;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Stream ended without message_stop — send final chunk
            if in_tool_use {
                tool_calls.push(MessageToolCall {
                    id: std::mem::take(&mut current_tool_id),
                    name: std::mem::take(&mut current_tool_name),
                    arguments: std::mem::take(&mut tool_args_buffer),
                });
            }
            let _ = tx
                .send(Ok(StreamChunk {
                    content: None,
                    tool_calls,
                    done: true,
                    usage: None,
                }))
                .await;
        });

        Ok(rx)
    }

    async fn list_models(&self) -> std::result::Result<Vec<String>, ProviderError> {
        // Anthropic doesn't have a models listing endpoint; return known models
        Ok(vec![
            "claude-sonnet-4-20250514".into(),
            "claude-opus-4-20250514".into(),
            "claude-haiku-35-20241022".into(),
            "claude-3-5-sonnet-20241022".into(),
        ])
    }

    async fn health_check(&self) -> std::result::Result<bool, ProviderError> {
        // Try a minimal request to verify API key
        let url = format!("{}/v1/messages", self.base_url);
        let body = serde_json::json!({
            "model": "claude-haiku-35-20241022",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 1,
        });

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        // 200 = works, 401 = bad key, anything else = reachable but error
        Ok(response.status().is_success() || response.status().as_u16() != 401)
    }
}

impl AnthropicProvider {
    /// Convert Anthropic API response to our ProviderResponse.
    fn response_to_provider_response(
        resp: AnthropicResponse,
    ) -> std::result::Result<ProviderResponse, ProviderError> {
        let mut text_content = String::new();
        let mut tool_calls = Vec::new();

        for block in &resp.content {
            match block {
                ResponseContentBlock::Text { text } => {
                    if !text_content.is_empty() {
                        text_content.push('\n');
                    }
                    text_content.push_str(text);
                }
                ResponseContentBlock::ToolUse { id, name, input } => {
                    tool_calls.push(MessageToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: serde_json::to_string(input).unwrap_or_default(),
                    });
                }
                ResponseContentBlock::Thinking { thinking } => {
                    // Prepend thinking to content with marker
                    text_content = format!("<thinking>{thinking}</thinking>\n{text_content}");
                }
            }
        }

        let message = Message {
            id: resp.id.clone(),
            role: Role::Assistant,
            content: text_content,
            tool_calls,
            tool_call_id: None,
            timestamp: chrono::Utc::now(),
            metadata: serde_json::Map::new(),
        };

        let usage = Some(Usage {
            prompt_tokens: resp.usage.input_tokens,
            completion_tokens: resp.usage.output_tokens,
            total_tokens: resp.usage.input_tokens + resp.usage.output_tokens,
        });

        Ok(ProviderResponse {
            message,
            usage,
            model: resp.model,
            metadata: serde_json::Map::new(),
        })
    }
}

// --- Anthropic API types ---

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: AnthropicContent,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum AnthropicContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    id: String,
    model: String,
    content: Vec<ResponseContentBlock>,
    usage: AnthropicUsage,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ResponseContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructor() {
        let provider = AnthropicProvider::new("sk-ant-test");
        assert_eq!(provider.name(), "anthropic");
        assert_eq!(provider.base_url, DEFAULT_BASE_URL);
        assert!(!provider.extended_thinking);
    }

    #[test]
    fn constructor_with_base_url() {
        let provider = AnthropicProvider::new("sk-ant-test")
            .with_base_url("https://custom.proxy.com/");
        assert_eq!(provider.base_url, "https://custom.proxy.com");
    }

    #[test]
    fn extended_thinking_config() {
        let provider = AnthropicProvider::new("sk-ant-test").with_extended_thinking(10000);
        assert!(provider.extended_thinking);
        assert_eq!(provider.thinking_budget, Some(10000));
    }

    #[test]
    fn system_extraction() {
        let messages = vec![
            Message::system("You are helpful"),
            Message::system("Be concise"),
            Message::user("Hello"),
            Message::assistant("Hi!"),
        ];

        let (system, non_system) = AnthropicProvider::extract_system(&messages);
        assert_eq!(system.as_deref(), Some("You are helpful\n\nBe concise"));
        assert_eq!(non_system.len(), 2);
        assert_eq!(non_system[0].role, Role::User);
        assert_eq!(non_system[1].role, Role::Assistant);
    }

    #[test]
    fn system_extraction_no_system() {
        let messages = vec![Message::user("Hello")];
        let (system, non_system) = AnthropicProvider::extract_system(&messages);
        assert!(system.is_none());
        assert_eq!(non_system.len(), 1);
    }

    #[test]
    fn message_conversion_user_assistant() {
        let messages = vec![Message::user("Hello"), Message::assistant("Hi!")];
        let refs: Vec<&Message> = messages.iter().collect();
        let api_msgs = AnthropicProvider::to_api_messages(&refs);
        assert_eq!(api_msgs.len(), 2);
        assert_eq!(api_msgs[0].role, "user");
        assert_eq!(api_msgs[1].role, "assistant");
    }

    #[test]
    fn message_conversion_with_tool_calls() {
        let mut msg = Message::assistant("Let me search");
        msg.tool_calls = vec![MessageToolCall {
            id: "toolu_123".into(),
            name: "web_search".into(),
            arguments: r#"{"query":"rust"}"#.into(),
        }];

        let refs: Vec<&Message> = vec![&msg];
        let api_msgs = AnthropicProvider::to_api_messages(&refs);
        assert_eq!(api_msgs.len(), 1);
        assert_eq!(api_msgs[0].role, "assistant");

        // Should be blocks, not text
        match &api_msgs[0].content {
            AnthropicContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 2); // text + tool_use
                match &blocks[0] {
                    ContentBlock::Text { text } => assert_eq!(text, "Let me search"),
                    _ => panic!("Expected text block"),
                }
                match &blocks[1] {
                    ContentBlock::ToolUse { id, name, .. } => {
                        assert_eq!(id, "toolu_123");
                        assert_eq!(name, "web_search");
                    }
                    _ => panic!("Expected tool_use block"),
                }
            }
            _ => panic!("Expected blocks content"),
        }
    }

    #[test]
    fn message_conversion_tool_result() {
        let msg = Message::tool_result("toolu_123", "search results here");
        let refs: Vec<&Message> = vec![&msg];
        let api_msgs = AnthropicProvider::to_api_messages(&refs);
        assert_eq!(api_msgs.len(), 1);
        assert_eq!(api_msgs[0].role, "user"); // Tool results go as user messages

        match &api_msgs[0].content {
            AnthropicContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                    } => {
                        assert_eq!(tool_use_id, "toolu_123");
                        assert_eq!(content, "search results here");
                    }
                    _ => panic!("Expected tool_result block"),
                }
            }
            _ => panic!("Expected blocks content"),
        }
    }

    #[test]
    fn tool_definition_conversion() {
        let tools = vec![ToolDefinition {
            name: "calculator".into(),
            description: "Evaluate math".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "expression": {"type": "string"}
                },
                "required": ["expression"]
            }),
        }];
        let api_tools = AnthropicProvider::to_api_tools(&tools);
        assert_eq!(api_tools.len(), 1);
        assert_eq!(api_tools[0].name, "calculator");
        assert_eq!(
            api_tools[0].input_schema["type"].as_str(),
            Some("object")
        );
    }

    #[test]
    fn parse_text_response() {
        let resp: AnthropicResponse = serde_json::from_str(
            r#"{
                "id": "msg_01",
                "model": "claude-sonnet-4-20250514",
                "content": [{"type": "text", "text": "Hello!"}],
                "usage": {"input_tokens": 10, "output_tokens": 5},
                "stop_reason": "end_turn"
            }"#,
        )
        .unwrap();

        let pr = AnthropicProvider::response_to_provider_response(resp).unwrap();
        assert_eq!(pr.message.content, "Hello!");
        assert!(pr.message.tool_calls.is_empty());
        assert_eq!(pr.usage.unwrap().total_tokens, 15);
        assert_eq!(pr.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn parse_tool_use_response() {
        let resp: AnthropicResponse = serde_json::from_str(
            r#"{
                "id": "msg_02",
                "model": "claude-sonnet-4-20250514",
                "content": [
                    {"type": "text", "text": "Let me calculate"},
                    {"type": "tool_use", "id": "toolu_abc", "name": "calculator", "input": {"expression": "2+2"}}
                ],
                "usage": {"input_tokens": 20, "output_tokens": 10},
                "stop_reason": "tool_use"
            }"#,
        )
        .unwrap();

        let pr = AnthropicProvider::response_to_provider_response(resp).unwrap();
        assert_eq!(pr.message.content, "Let me calculate");
        assert_eq!(pr.message.tool_calls.len(), 1);
        assert_eq!(pr.message.tool_calls[0].name, "calculator");
        assert_eq!(pr.message.tool_calls[0].id, "toolu_abc");
        let args: serde_json::Value =
            serde_json::from_str(&pr.message.tool_calls[0].arguments).unwrap();
        assert_eq!(args["expression"], "2+2");
    }

    #[test]
    fn parse_thinking_response() {
        let resp: AnthropicResponse = serde_json::from_str(
            r#"{
                "id": "msg_03",
                "model": "claude-sonnet-4-20250514",
                "content": [
                    {"type": "thinking", "thinking": "I need to consider..."},
                    {"type": "text", "text": "Here's my answer."}
                ],
                "usage": {"input_tokens": 15, "output_tokens": 25}
            }"#,
        )
        .unwrap();

        let pr = AnthropicProvider::response_to_provider_response(resp).unwrap();
        assert!(pr.message.content.contains("<thinking>"));
        assert!(pr.message.content.contains("Here's my answer."));
    }

    #[test]
    fn anthropic_content_serialization() {
        let msg = AnthropicMessage {
            role: "user".into(),
            content: AnthropicContent::Text("Hello".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"Hello\""));

        let msg2 = AnthropicMessage {
            role: "assistant".into(),
            content: AnthropicContent::Blocks(vec![ContentBlock::Text {
                text: "Hi".into(),
            }]),
        };
        let json2 = serde_json::to_string(&msg2).unwrap();
        assert!(json2.contains("\"type\":\"text\""));
    }

    #[test]
    fn list_models_returns_known_models() {
        let provider = AnthropicProvider::new("sk-test");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let models = rt.block_on(provider.list_models()).unwrap();
        assert!(models.len() >= 3);
        assert!(models.iter().any(|m| m.contains("claude")));
    }
}

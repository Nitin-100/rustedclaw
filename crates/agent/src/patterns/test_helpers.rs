//! Shared test helpers for pattern tests.

use rustedclaw_core::error::ProviderError;
use rustedclaw_core::message::Message;
use rustedclaw_core::provider::{Provider, ProviderRequest, ProviderResponse, Usage};
use std::sync::Mutex;

/// A mock provider that returns a sequence of scripted responses.
///
/// Each call to `complete` returns the next response in the queue.
/// Panics if more calls are made than responses provided.
pub struct SequentialMockProvider {
    responses: Mutex<Vec<ProviderResponse>>,
    call_count: Mutex<usize>,
}

impl SequentialMockProvider {
    pub fn new(responses: Vec<ProviderResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
            call_count: Mutex::new(0),
        }
    }

    /// Create a provider that returns a single text response (no tool calls).
    pub fn single_text(text: &str) -> Self {
        Self::new(vec![make_text_response(text)])
    }

    /// Create a provider that first returns tool calls, then a final answer.
    pub fn tool_then_answer(
        tool_calls: Vec<rustedclaw_core::message::MessageToolCall>,
        thought: &str,
        answer: &str,
    ) -> Self {
        Self::new(vec![
            make_tool_call_response(tool_calls, thought),
            make_text_response(answer),
        ])
    }

    #[allow(dead_code)]
    pub fn call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }
}

#[async_trait::async_trait]
impl Provider for SequentialMockProvider {
    fn name(&self) -> &str {
        "sequential_mock"
    }

    async fn complete(&self, _request: ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let mut count = self.call_count.lock().unwrap();
        let responses = self.responses.lock().unwrap();

        if *count >= responses.len() {
            panic!(
                "SequentialMockProvider: no more responses (call #{}, have {})",
                *count,
                responses.len()
            );
        }

        let response = responses[*count].clone();
        *count += 1;
        Ok(response)
    }
}

/// Create a simple text response (no tool calls).
pub fn make_text_response(text: &str) -> ProviderResponse {
    ProviderResponse {
        message: Message::assistant(text),
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
        model: "mock-model".into(),
        metadata: serde_json::Map::new(),
    }
}

/// Create a response with tool calls and optional thought content.
pub fn make_tool_call_response(
    tool_calls: Vec<rustedclaw_core::message::MessageToolCall>,
    thought: &str,
) -> ProviderResponse {
    let mut msg = Message::assistant(thought);
    msg.tool_calls = tool_calls;
    ProviderResponse {
        message: msg,
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
        model: "mock-model".into(),
        metadata: serde_json::Map::new(),
    }
}

/// Helper to create a tool call.
pub fn make_tool_call(
    name: &str,
    args: serde_json::Value,
) -> rustedclaw_core::message::MessageToolCall {
    rustedclaw_core::message::MessageToolCall {
        id: format!("call_{}", name),
        name: name.to_string(),
        arguments: serde_json::to_string(&args).unwrap(),
    }
}

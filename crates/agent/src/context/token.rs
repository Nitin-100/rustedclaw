//! Token estimation utilities.
//!
//! Uses a character-based heuristic: ~4 characters per token.
//! This approximation is accurate within ~10% for BPE tokenizers
//! (GPT-3.5, GPT-4, Claude) on English text. The trial spec sets
//! 4096 tokens as the default budget, keeping test cases predictable.

use rustedclaw_core::message::Message;
use rustedclaw_core::provider::ToolDefinition;

/// Estimate the token count for a string.
///
/// Heuristic: 1 token ≈ 4 characters. Rounds up.
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    (text.len() + 3) / 4
}

/// Estimate tokens for a single message including per-message overhead.
///
/// Each message costs ~4 tokens of overhead for role name, delimiters,
/// and formatting markers in the API wire format.
pub fn estimate_message_tokens(message: &Message) -> usize {
    let overhead = 4;
    overhead + estimate_tokens(&message.content)
}

/// Estimate tokens for a slice of messages.
pub fn estimate_messages_tokens(messages: &[Message]) -> usize {
    messages.iter().map(|m| estimate_message_tokens(m)).sum()
}

/// Estimate tokens for a tool definition (serialized as JSON).
pub fn estimate_tool_tokens(tool: &ToolDefinition) -> usize {
    let json = serde_json::to_string(tool).unwrap_or_default();
    estimate_tokens(&json)
}

/// Estimate tokens for a slice of tool definitions.
pub fn estimate_tools_tokens(tools: &[ToolDefinition]) -> usize {
    tools.iter().map(|t| estimate_tool_tokens(t)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_is_zero() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn four_chars_is_one_token() {
        assert_eq!(estimate_tokens("test"), 1);
    }

    #[test]
    fn five_chars_rounds_up() {
        assert_eq!(estimate_tokens("hello"), 2);
    }

    #[test]
    fn hundred_chars() {
        let text = "a".repeat(100);
        assert_eq!(estimate_tokens(&text), 25);
    }

    #[test]
    fn message_includes_overhead() {
        let msg = Message::user("test"); // 4 chars → 1 token + 4 overhead = 5
        assert_eq!(estimate_message_tokens(&msg), 5);
    }

    #[test]
    fn multiple_messages() {
        let msgs = vec![
            Message::user("hello"),     // 5 chars → 2 tokens + 4 overhead = 6
            Message::assistant("world"), // 5 chars → 2 tokens + 4 overhead = 6
        ];
        assert_eq!(estimate_messages_tokens(&msgs), 12);
    }

    #[test]
    fn tool_definition_tokens() {
        let tool = ToolDefinition {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                }
            }),
        };
        let tokens = estimate_tool_tokens(&tool);
        assert!(tokens > 0);
    }

    #[test]
    fn empty_tools_is_zero() {
        assert_eq!(estimate_tools_tokens(&[]), 0);
    }
}

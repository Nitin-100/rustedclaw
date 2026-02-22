//! Agent-level streaming events.
//!
//! `AgentStreamEvent` wraps provider-level stream chunks into higher-level
//! events that the gateway can forward to clients over SSE or WebSocket.

use rustedclaw_core::provider::Usage;
use serde::{Deserialize, Serialize};

/// Events emitted by the agent during streaming execution.
///
/// These follow the PRD-defined WebSocket protocol:
/// - `chunk`       — partial text token from the LLM
/// - `tool_call`   — agent is invoking a tool
/// - `tool_result` — tool execution completed
/// - `thought`     — ReAct reasoning step
/// - `done`        — stream is complete
/// - `error`       — an error occurred
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentStreamEvent {
    /// Partial text token from the LLM.
    Chunk { content: String },

    /// The agent is calling a tool.
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    /// Tool execution completed.
    ToolResult {
        id: String,
        name: String,
        output: String,
        success: bool,
    },

    /// A thought / reasoning step (ReAct trace).
    Thought { content: String },

    /// The stream is complete — final metadata.
    Done {
        conversation_id: String,
        usage: Option<Usage>,
        iterations: usize,
        tool_calls_made: usize,
    },

    /// An error occurred mid-stream.
    Error { message: String },
}

impl AgentStreamEvent {
    /// SSE event name for this event type.
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::Chunk { .. } => "chunk",
            Self::ToolCall { .. } => "tool_call",
            Self::ToolResult { .. } => "tool_result",
            Self::Thought { .. } => "thought",
            Self::Done { .. } => "done",
            Self::Error { .. } => "error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_serialization_chunk() {
        let event = AgentStreamEvent::Chunk {
            content: "Hello".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"chunk""#));
        assert!(json.contains(r#""content":"Hello""#));
    }

    #[test]
    fn event_serialization_tool_call() {
        let event = AgentStreamEvent::ToolCall {
            id: "call_1".into(),
            name: "calculator".into(),
            input: serde_json::json!({"expr": "2+2"}),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"tool_call""#));
        assert!(json.contains(r#""name":"calculator""#));
    }

    #[test]
    fn event_serialization_done() {
        let event = AgentStreamEvent::Done {
            conversation_id: "abc".into(),
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
            }),
            iterations: 2,
            tool_calls_made: 1,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"done""#));
        assert!(json.contains(r#""iterations":2"#));
    }

    #[test]
    fn event_serialization_error() {
        let event = AgentStreamEvent::Error {
            message: "boom".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"error""#));
    }

    #[test]
    fn event_type_names() {
        assert_eq!(
            AgentStreamEvent::Chunk {
                content: "x".into()
            }
            .event_type(),
            "chunk"
        );
        assert_eq!(
            AgentStreamEvent::ToolCall {
                id: "a".into(),
                name: "b".into(),
                input: serde_json::Value::Null
            }
            .event_type(),
            "tool_call"
        );
        assert_eq!(
            AgentStreamEvent::ToolResult {
                id: "a".into(),
                name: "b".into(),
                output: "c".into(),
                success: true
            }
            .event_type(),
            "tool_result"
        );
        assert_eq!(
            AgentStreamEvent::Thought {
                content: "x".into()
            }
            .event_type(),
            "thought"
        );
        assert_eq!(
            AgentStreamEvent::Done {
                conversation_id: "x".into(),
                usage: None,
                iterations: 0,
                tool_calls_made: 0
            }
            .event_type(),
            "done"
        );
        assert_eq!(
            AgentStreamEvent::Error {
                message: "x".into()
            }
            .event_type(),
            "error"
        );
    }

    #[test]
    fn event_deserialization() {
        let json = r#"{"type":"chunk","content":"hi"}"#;
        let event: AgentStreamEvent = serde_json::from_str(json).unwrap();
        match event {
            AgentStreamEvent::Chunk { content } => assert_eq!(content, "hi"),
            _ => panic!("Wrong variant"),
        }
    }
}

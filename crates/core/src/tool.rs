//! Tool trait — the abstraction over agent capabilities.
//!
//! Tools are what give the agent the ability to act in the world:
//! execute shell commands, read/write files, search the web, etc.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::error::ToolError;
use crate::provider::ToolDefinition;

/// A request to execute a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique call ID (matches the LLM's tool_call.id)
    pub id: String,

    /// Name of the tool to execute
    pub name: String,

    /// Arguments as a JSON value
    pub arguments: serde_json::Value,
}

/// The result of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// The call ID this result is for
    pub call_id: String,

    /// Whether the tool executed successfully
    pub success: bool,

    /// The output content
    pub output: String,

    /// Optional structured data
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// The core Tool trait.
///
/// Each tool (shell, file_read, file_write, http_request, memory_search, etc.)
/// implements this trait. Tools are registered in the ToolRegistry and made
/// available to the agent loop.
#[async_trait]
pub trait Tool: Send + Sync {
    /// The unique name of this tool (e.g., "shell", "file_read").
    fn name(&self) -> &str;

    /// A description of what this tool does (sent to the LLM).
    fn description(&self) -> &str;

    /// JSON Schema describing this tool's parameters.
    fn parameters_schema(&self) -> serde_json::Value;

    /// Execute the tool with the given arguments.
    async fn execute(&self, arguments: serde_json::Value) -> std::result::Result<ToolResult, ToolError>;

    /// Convert this tool into a ToolDefinition for sending to the LLM.
    fn to_definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters_schema(),
        }
    }
}

/// A registry of available tools.
///
/// The agent loop uses this to:
/// 1. Get tool definitions to send to the LLM
/// 2. Look up and execute tools when the LLM requests them
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool. Replaces any existing tool with the same name.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    /// Get a tool by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// Get all tool definitions (for sending to the LLM).
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|t| t.to_definition()).collect()
    }

    /// Execute a tool call.
    pub async fn execute(&self, call: &ToolCall) -> std::result::Result<ToolResult, ToolError> {
        let tool = self.tools.get(&call.name).ok_or_else(|| ToolError::NotFound(call.name.clone()))?;
        tool.execute(call.arguments.clone()).await
    }

    /// List all registered tool names.
    pub fn names(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A simple test tool for unit tests.
    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str { "echo" }
        fn description(&self) -> &str { "Echoes back the input" }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string" }
                },
                "required": ["text"]
            })
        }
        async fn execute(&self, arguments: serde_json::Value) -> std::result::Result<ToolResult, ToolError> {
            let text = arguments["text"].as_str().unwrap_or("").to_string();
            Ok(ToolResult {
                call_id: "test".into(),
                success: true,
                output: text,
                data: None,
            })
        }
    }

    #[test]
    fn registry_register_and_lookup() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool));
        assert!(registry.get("echo").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn registry_definitions() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool));
        let defs = registry.definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "echo");
    }

    #[tokio::test]
    async fn registry_execute_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool));

        let call = ToolCall {
            id: "call_1".into(),
            name: "echo".into(),
            arguments: serde_json::json!({"text": "hello world"}),
        };
        let result = registry.execute(&call).await.unwrap();
        assert!(result.success);
        assert_eq!(result.output, "hello world");
    }

    #[tokio::test]
    async fn registry_execute_missing_tool() {
        let registry = ToolRegistry::new();
        let call = ToolCall {
            id: "call_1".into(),
            name: "nonexistent".into(),
            arguments: serde_json::json!({}),
        };
        let err = registry.execute(&call).await.unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }
}

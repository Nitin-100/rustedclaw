//! Memory search tool — allows the agent to explicitly search its long-term memory.
//!
//! This tool bridges the tools system with the memory system, giving the
//! agent the ability to search its own stored memories, facts, and past
//! conversations on demand.
//!
//! When no `MemoryBackend` is injected, the tool returns stub/mock results.
//! When a real backend is provided, it delegates to `MemoryBackend::search()`.

use async_trait::async_trait;
use rustedclaw_core::error::ToolError;
use rustedclaw_core::memory::{MemoryBackend, MemoryQuery, SearchMode};
use rustedclaw_core::tool::{Tool, ToolResult};
use std::sync::Arc;

/// A tool that searches the agent's long-term memory store.
///
/// Without a backend, returns mock memory entries for testing.
/// With a backend, performs real memory searches.
pub struct MemorySearchTool {
    backend: Option<Arc<dyn MemoryBackend>>,
}

impl MemorySearchTool {
    /// Create a new memory search tool without a backend (stub mode).
    pub fn new() -> Self {
        Self { backend: None }
    }

    /// Create a memory search tool backed by a real memory backend.
    pub fn with_backend(backend: Arc<dyn MemoryBackend>) -> Self {
        Self {
            backend: Some(backend),
        }
    }
}

impl Default for MemorySearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        "memory_search"
    }

    fn description(&self) -> &str {
        "Search your long-term memory for relevant facts, past conversations, and stored knowledge. \
         Use this when you need to recall something you've learned or been told before."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query to find relevant memories"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of memories to return (default 5)",
                    "default": 5
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional tags to filter memories by"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<ToolResult, ToolError> {
        let query = arguments["query"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("Missing 'query' argument".into()))?;

        let limit = arguments["limit"].as_u64().unwrap_or(5).min(50) as usize;

        let _tags: Vec<String> = arguments
            .get("tags")
            .and_then(|t| serde_json::from_value(t.clone()).ok())
            .unwrap_or_default();

        // If we have a real backend, use it
        if let Some(backend) = &self.backend {
            let search_query = MemoryQuery {
                text: query.to_string(),
                limit,
                min_score: 0.0,
                tags: vec![],
                mode: SearchMode::Keyword,
            };

            match backend.search(search_query).await {
                Ok(entries) => {
                    let results: Vec<MemoryResult> = entries
                        .into_iter()
                        .map(|e| MemoryResult {
                            id: e.id,
                            content: e.content,
                            tags: e.tags,
                            score: e.score as f64,
                            source: e.source,
                            created_at: Some(e.created_at.to_rfc3339()),
                        })
                        .collect();

                    let output = if results.is_empty() {
                        format!("No memories found matching '{query}'.")
                    } else {
                        serde_json::to_string_pretty(&results).unwrap_or_default()
                    };

                    Ok(ToolResult {
                        call_id: String::new(),
                        success: true,
                        output,
                        data: Some(serde_json::to_value(&results).unwrap()),
                    })
                }
                Err(e) => Ok(ToolResult {
                    call_id: String::new(),
                    success: false,
                    output: format!("Memory search failed: {e}"),
                    data: None,
                }),
            }
        } else {
            // Stub mode: return mock memories
            let results = generate_mock_memories(query, limit);
            let output = if results.is_empty() {
                format!("No memories found matching '{query}'.")
            } else {
                serde_json::to_string_pretty(&results).unwrap_or_default()
            };

            Ok(ToolResult {
                call_id: String::new(),
                success: true,
                output,
                data: Some(serde_json::to_value(&results).unwrap()),
            })
        }
    }
}

#[derive(serde::Serialize)]
struct MemoryResult {
    id: String,
    content: String,
    tags: Vec<String>,
    score: f64,
    source: Option<String>,
    created_at: Option<String>,
}

fn generate_mock_memories(query: &str, limit: usize) -> Vec<MemoryResult> {
    let q = query.to_lowercase();

    // Context-aware mock memories
    let mut results = Vec::new();

    if q.contains("favorite") || q.contains("preference") {
        results.push(MemoryResult {
            id: "mem_pref_001".into(),
            content: "User's favorite programming language is Rust.".into(),
            tags: vec!["preference".into(), "programming".into()],
            score: 0.92,
            source: Some("conversation".into()),
            created_at: Some("2024-01-15T10:30:00Z".into()),
        });
        results.push(MemoryResult {
            id: "mem_pref_002".into(),
            content: "User prefers dark mode in all applications.".into(),
            tags: vec!["preference".into(), "ui".into()],
            score: 0.85,
            source: Some("conversation".into()),
            created_at: Some("2024-01-14T08:15:00Z".into()),
        });
    }

    if q.contains("project") || q.contains("work") {
        results.push(MemoryResult {
            id: "mem_proj_001".into(),
            content: "User is working on an AI agent framework called RustedClaw.".into(),
            tags: vec!["project".into(), "ai".into()],
            score: 0.95,
            source: Some("conversation".into()),
            created_at: Some("2024-02-01T14:00:00Z".into()),
        });
    }

    if q.contains("name") || q.contains("who") {
        results.push(MemoryResult {
            id: "mem_identity_001".into(),
            content: "User's name is Alex.".into(),
            tags: vec!["identity".into()],
            score: 0.99,
            source: Some("introduction".into()),
            created_at: Some("2024-01-01T00:00:00Z".into()),
        });
    }

    // If no specific matches, generate generic results
    if results.is_empty() {
        for i in 0..limit.min(3) {
            results.push(MemoryResult {
                id: format!("mem_generic_{:03}", i + 1),
                content: format!(
                    "This is a mock memory entry related to '{}'. In production, this would \
                     contain actual stored knowledge.",
                    query
                ),
                tags: vec!["auto-saved".into()],
                score: 0.7 - (i as f64 * 0.1),
                source: Some("conversation".into()),
                created_at: Some("2024-01-10T12:00:00Z".into()),
            });
        }
    }

    results.into_iter().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustedclaw_core::memory::MemoryEntry;
    use rustedclaw_memory::InMemoryBackend;

    #[test]
    fn tool_definition() {
        let tool = MemorySearchTool::new();
        assert_eq!(tool.name(), "memory_search");
        let schema = tool.parameters_schema();
        assert_eq!(schema["required"], serde_json::json!(["query"]));
        assert!(schema["properties"]["limit"].is_object());
        assert!(schema["properties"]["tags"].is_object());
    }

    #[tokio::test]
    async fn stub_returns_mock_memories() {
        let tool = MemorySearchTool::new();
        let result = tool
            .execute(serde_json::json!({
                "query": "favorite programming language"
            }))
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.contains("Rust"));
        let data = result.data.unwrap();
        let entries: Vec<serde_json::Value> = serde_json::from_value(data).unwrap();
        assert!(!entries.is_empty());
    }

    #[tokio::test]
    async fn stub_project_query() {
        let tool = MemorySearchTool::new();
        let result = tool
            .execute(serde_json::json!({
                "query": "current project"
            }))
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.contains("RustedClaw"));
    }

    #[tokio::test]
    async fn stub_generic_query() {
        let tool = MemorySearchTool::new();
        let result = tool
            .execute(serde_json::json!({
                "query": "something random",
                "limit": 2
            }))
            .await
            .unwrap();

        assert!(result.success);
        let data = result.data.unwrap();
        let entries: Vec<serde_json::Value> = serde_json::from_value(data).unwrap();
        assert!(entries.len() <= 2);
    }

    #[tokio::test]
    async fn missing_query_returns_error() {
        let tool = MemorySearchTool::new();
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn with_real_backend_searches_memory() {
        let backend = Arc::new(InMemoryBackend::new());
        backend
            .store(MemoryEntry {
                id: "test_001".into(),
                content: "The capital of France is Paris".into(),
                tags: vec!["geography".into()],
                source: Some("test".into()),
                created_at: chrono::Utc::now(),
                last_accessed: chrono::Utc::now(),
                score: 0.0,
                embedding: None,
            })
            .await
            .unwrap();

        let tool = MemorySearchTool::with_backend(backend);
        let result = tool
            .execute(serde_json::json!({
                "query": "capital of France"
            }))
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.contains("Paris"));
    }

    #[tokio::test]
    async fn with_real_backend_no_results() {
        let backend = Arc::new(InMemoryBackend::new());
        let tool = MemorySearchTool::with_backend(backend);
        let result = tool
            .execute(serde_json::json!({
                "query": "nonexistent topic xyz"
            }))
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.contains("No memories found"));
    }

    #[tokio::test]
    async fn default_constructor() {
        let tool = MemorySearchTool::default();
        assert_eq!(tool.name(), "memory_search");
        assert!(tool.backend.is_none());
    }
}

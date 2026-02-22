//! Knowledge base query tool — stub for RAG retrieval.
//!
//! In production this would perform vector similarity search against
//! a PostgreSQL + pgvector database. The stub returns mock knowledge
//! chunks so the RAG agent pattern can be tested end-to-end.

use async_trait::async_trait;
use rustedclaw_core::error::ToolError;
use rustedclaw_core::tool::{Tool, ToolResult};

pub struct KnowledgeBaseQueryTool;

#[async_trait]
impl Tool for KnowledgeBaseQueryTool {
    fn name(&self) -> &str {
        "knowledge_base_query"
    }

    fn description(&self) -> &str {
        "Query the knowledge base for relevant information. Returns document chunks sorted by relevance."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query to find relevant knowledge"
                },
                "top_k": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default 3)",
                    "default": 3
                },
                "min_score": {
                    "type": "number",
                    "description": "Minimum similarity score threshold (0.0-1.0, default 0.5)",
                    "default": 0.5
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<ToolResult, ToolError> {
        let query = arguments["query"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("Missing 'query' argument".into()))?;

        let top_k = arguments["top_k"].as_u64().unwrap_or(3).min(10) as usize;

        let chunks = generate_mock_chunks(query, top_k);
        let output = serde_json::to_string_pretty(&chunks).unwrap_or_default();

        Ok(ToolResult {
            call_id: String::new(),
            success: true,
            output,
            data: Some(serde_json::to_value(&chunks).unwrap()),
        })
    }
}

#[derive(serde::Serialize)]
struct KnowledgeResult {
    document_id: String,
    chunk_index: usize,
    content: String,
    source: String,
    similarity: f64,
}

fn generate_mock_chunks(query: &str, top_k: usize) -> Vec<KnowledgeResult> {
    let q = query.to_lowercase();

    // Topic-specific mock knowledge for realistic agent testing.
    let knowledge_base: Vec<(&str, Vec<KnowledgeResult>)> = vec![
        ("rust", vec![
            KnowledgeResult {
                document_id: "doc_rust_001".into(),
                chunk_index: 0,
                content: "Rust is a multi-paradigm, general-purpose programming language that emphasizes performance, type safety, and concurrency. It enforces memory safety without a garbage collector.".into(),
                source: "rust_overview.md".into(),
                similarity: 0.95,
            },
            KnowledgeResult {
                document_id: "doc_rust_001".into(),
                chunk_index: 1,
                content: "Rust's ownership system guarantees memory safety and thread safety at compile time, eliminating data races. The borrow checker enforces these rules.".into(),
                source: "rust_overview.md".into(),
                similarity: 0.88,
            },
            KnowledgeResult {
                document_id: "doc_rust_002".into(),
                chunk_index: 0,
                content: "Cargo is Rust's build system and package manager. It downloads dependencies, compiles packages, and can upload to crates.io.".into(),
                source: "rust_tooling.md".into(),
                similarity: 0.75,
            },
        ]),
        ("wasm", vec![
            KnowledgeResult {
                document_id: "doc_wasm_001".into(),
                chunk_index: 0,
                content: "WebAssembly (WASM) is a binary instruction format for a stack-based virtual machine. It enables high-performance applications on the web and beyond.".into(),
                source: "wasm_intro.md".into(),
                similarity: 0.93,
            },
            KnowledgeResult {
                document_id: "doc_wasm_002".into(),
                chunk_index: 0,
                content: "The WASM Component Model defines a portable, language-agnostic binary format with a rich type system (WIT). Components can be composed and linked.".into(),
                source: "component_model.md".into(),
                similarity: 0.85,
            },
        ]),
        ("agent", vec![
            KnowledgeResult {
                document_id: "doc_agent_001".into(),
                chunk_index: 0,
                content: "The ReAct pattern combines reasoning and acting in an interleaved manner. The agent thinks (Thought), acts (Action), and observes (Observation) in a loop.".into(),
                source: "agent_patterns.md".into(),
                similarity: 0.92,
            },
            KnowledgeResult {
                document_id: "doc_agent_002".into(),
                chunk_index: 0,
                content: "RAG (Retrieval-Augmented Generation) grounds LLM responses in factual data by retrieving relevant documents before generating answers.".into(),
                source: "rag_pattern.md".into(),
                similarity: 0.87,
            },
        ]),
    ];

    // Match topics or return generic results.
    for (keyword, results) in &knowledge_base {
        if q.contains(keyword) {
            return results.iter().take(top_k).cloned().collect();
        }
    }

    // Generic fallback with decreasing similarity.
    (0..top_k)
        .map(|i| KnowledgeResult {
            document_id: format!("doc_gen_{:03}", i),
            chunk_index: 0,
            content: format!(
                "Knowledge chunk {} related to '{}'. This is mock content for testing the RAG pipeline.",
                i + 1,
                query
            ),
            source: format!("knowledge_{}.md", i + 1),
            similarity: 0.9 - (i as f64 * 0.1),
        })
        .collect()
}

impl Clone for KnowledgeResult {
    fn clone(&self) -> Self {
        Self {
            document_id: self.document_id.clone(),
            chunk_index: self.chunk_index,
            content: self.content.clone(),
            source: self.source.clone(),
            similarity: self.similarity,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn query_returns_results() {
        let tool = KnowledgeBaseQueryTool;
        let result = tool
            .execute(serde_json::json!({"query": "rust ownership"}))
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.contains("Rust"));
        assert!(result.output.contains("similarity"));
    }

    #[tokio::test]
    async fn respects_top_k() {
        let tool = KnowledgeBaseQueryTool;
        let result = tool
            .execute(serde_json::json!({"query": "test", "top_k": 2}))
            .await
            .unwrap();

        let data: Vec<serde_json::Value> = serde_json::from_str(&result.output).unwrap();
        assert_eq!(data.len(), 2);
    }

    #[tokio::test]
    async fn generic_query_works() {
        let tool = KnowledgeBaseQueryTool;
        let result = tool
            .execute(serde_json::json!({"query": "some random topic"}))
            .await
            .unwrap();

        assert!(result.success);
        let data: Vec<serde_json::Value> = serde_json::from_str(&result.output).unwrap();
        assert_eq!(data.len(), 3); // default top_k
    }

    #[tokio::test]
    async fn missing_query_returns_error() {
        let tool = KnowledgeBaseQueryTool;
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[test]
    fn tool_definition() {
        let tool = KnowledgeBaseQueryTool;
        let def = tool.to_definition();
        assert_eq!(def.name, "knowledge_base_query");
    }
}

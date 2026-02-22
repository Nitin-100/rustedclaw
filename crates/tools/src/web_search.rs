//! Web search tool — stub that returns mock search results.
//!
//! In production this would call a real search API (Brave, Google, etc.).
//! The stub returns plausible results so the agent loop and ReAct pattern
//! can be tested end-to-end without network access.

use async_trait::async_trait;
use rustedclaw_core::error::ToolError;
use rustedclaw_core::tool::{Tool, ToolResult};

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web for information. Returns a list of relevant results with titles, URLs, and snippets."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                },
                "num_results": {
                    "type": "integer",
                    "description": "Number of results to return (default 3)",
                    "default": 3
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<ToolResult, ToolError> {
        let query = arguments["query"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("Missing 'query' argument".into()))?;

        let num_results = arguments["num_results"].as_u64().unwrap_or(3).min(5) as usize;

        // Generate deterministic mock results based on query content.
        let results = generate_mock_results(query, num_results);
        let output = serde_json::to_string_pretty(&results).unwrap_or_default();

        Ok(ToolResult {
            call_id: String::new(),
            success: true,
            output,
            data: Some(serde_json::to_value(&results).unwrap()),
        })
    }
}

#[derive(serde::Serialize)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

fn generate_mock_results(query: &str, count: usize) -> Vec<SearchResult> {
    let q = query.to_lowercase();

    // Provide context-aware mock results for common topics.
    let templates: Vec<(&str, Vec<SearchResult>)> = vec![
        ("rust", vec![
            SearchResult {
                title: "The Rust Programming Language".into(),
                url: "https://doc.rust-lang.org/book/".into(),
                snippet: "Rust is a systems programming language focused on safety, speed, and concurrency.".into(),
            },
            SearchResult {
                title: "Rust by Example".into(),
                url: "https://doc.rust-lang.org/rust-by-example/".into(),
                snippet: "A collection of runnable examples that illustrate Rust concepts and standard library usage.".into(),
            },
            SearchResult {
                title: "crates.io: Rust Package Registry".into(),
                url: "https://crates.io/".into(),
                snippet: "The Rust community's crate registry for sharing and discovering Rust libraries.".into(),
            },
        ]),
        ("weather", vec![
            SearchResult {
                title: "Weather Forecast - National Weather Service".into(),
                url: "https://weather.gov/".into(),
                snippet: "Current conditions and forecasts for locations across the United States.".into(),
            },
            SearchResult {
                title: "OpenWeatherMap".into(),
                url: "https://openweathermap.org/".into(),
                snippet: "Free weather API providing current weather data and forecasts for any location.".into(),
            },
        ]),
    ];

    // Find matching template or generate generic results.
    for (keyword, results) in &templates {
        if q.contains(keyword) {
            return results.iter().take(count).cloned().collect();
        }
    }

    // Generic fallback.
    (0..count)
        .map(|i| SearchResult {
            title: format!("Result {} for: {}", i + 1, query),
            url: format!("https://example.com/search?q={}&p={}", urlencod(query), i + 1),
            snippet: format!(
                "This is a mock search result for the query '{}'. In production, this would contain real content.",
                query
            ),
        })
        .collect()
}

fn urlencod(s: &str) -> String {
    s.replace(' ', "+")
}

impl Clone for SearchResult {
    fn clone(&self) -> Self {
        Self {
            title: self.title.clone(),
            url: self.url.clone(),
            snippet: self.snippet.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn search_returns_results() {
        let tool = WebSearchTool;
        let result = tool
            .execute(serde_json::json!({"query": "rust programming"}))
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.contains("Rust"));
        assert!(result.data.is_some());
    }

    #[tokio::test]
    async fn search_respects_num_results() {
        let tool = WebSearchTool;
        let result = tool
            .execute(serde_json::json!({"query": "test", "num_results": 2}))
            .await
            .unwrap();

        let data: Vec<serde_json::Value> = serde_json::from_str(&result.output).unwrap();
        assert_eq!(data.len(), 2);
    }

    #[tokio::test]
    async fn missing_query_returns_error() {
        let tool = WebSearchTool;
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[test]
    fn tool_definition() {
        let tool = WebSearchTool;
        let def = tool.to_definition();
        assert_eq!(def.name, "web_search");
        assert!(!def.description.is_empty());
    }
}

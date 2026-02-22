//! HTTP request tool — stub that returns mock HTTP responses.
//!
//! In production this would use `reqwest` to make real HTTP calls.
//! The stub returns realistic mock responses so the agent loop and
//! routines can be tested end-to-end without network access.

use async_trait::async_trait;
use rustedclaw_core::error::ToolError;
use rustedclaw_core::tool::{Tool, ToolResult};
use std::collections::HashMap;

pub struct HttpRequestTool;

#[async_trait]
impl Tool for HttpRequestTool {
    fn name(&self) -> &str {
        "http_request"
    }

    fn description(&self) -> &str {
        "Make an HTTP request to a URL. Supports GET, POST, PUT, PATCH, and DELETE methods. \
         Returns the response status code, headers, and body."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to send the request to"
                },
                "method": {
                    "type": "string",
                    "description": "HTTP method (GET, POST, PUT, PATCH, DELETE). Defaults to GET.",
                    "enum": ["GET", "POST", "PUT", "PATCH", "DELETE"],
                    "default": "GET"
                },
                "headers": {
                    "type": "object",
                    "description": "Optional HTTP headers as key-value pairs",
                    "additionalProperties": { "type": "string" }
                },
                "body": {
                    "type": "string",
                    "description": "Optional request body (for POST, PUT, PATCH)"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Request timeout in seconds (default 30)",
                    "default": 30
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<ToolResult, ToolError> {
        let url = arguments["url"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("Missing 'url' argument".into()))?;

        let method = arguments["method"]
            .as_str()
            .unwrap_or("GET")
            .to_uppercase();

        // Validate method
        if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
            return Err(ToolError::InvalidArguments(format!(
                "Invalid HTTP method: {method}. Must be GET, POST, PUT, PATCH, or DELETE."
            )));
        }

        // Validate URL format
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(ToolError::InvalidArguments(
                "URL must start with http:// or https://".into(),
            ));
        }

        let headers: HashMap<String, String> = arguments
            .get("headers")
            .and_then(|h| serde_json::from_value(h.clone()).ok())
            .unwrap_or_default();

        let body = arguments["body"].as_str().map(|s| s.to_string());

        let _timeout_secs = arguments["timeout_secs"].as_u64().unwrap_or(30);

        // Generate mock response based on URL patterns
        let response = generate_mock_response(url, &method, &headers, body.as_deref());
        let output = serde_json::to_string_pretty(&response).unwrap_or_default();

        Ok(ToolResult {
            call_id: String::new(),
            success: response.status_code < 400,
            output,
            data: Some(serde_json::to_value(&response).unwrap()),
        })
    }
}

#[derive(serde::Serialize)]
struct HttpResponse {
    status_code: u16,
    status_text: String,
    headers: HashMap<String, String>,
    body: String,
    elapsed_ms: u64,
}

fn generate_mock_response(
    url: &str,
    method: &str,
    _request_headers: &HashMap<String, String>,
    request_body: Option<&str>,
) -> HttpResponse {
    let mut response_headers = HashMap::new();
    response_headers.insert("content-type".into(), "application/json".into());
    response_headers.insert("x-mock".into(), "true".into());

    // Pattern-match URL to generate context-aware responses
    let lower_url = url.to_lowercase();

    if lower_url.contains("/health") || lower_url.contains("/healthz") {
        return HttpResponse {
            status_code: 200,
            status_text: "OK".into(),
            headers: response_headers,
            body: r#"{"status":"healthy","uptime_secs":86400}"#.into(),
            elapsed_ms: 12,
        };
    }

    if lower_url.contains("/api/") || lower_url.contains("/v1/") {
        let body = match method {
            "GET" => r#"{"data":[{"id":1,"name":"Item 1"},{"id":2,"name":"Item 2"}],"total":2}"#.into(),
            "POST" => {
                let id = simple_hash(request_body.unwrap_or("")) % 10000;
                format!(r#"{{"id":{id},"created":true,"message":"Resource created successfully"}}"#)
            }
            "PUT" | "PATCH" => r#"{"updated":true,"message":"Resource updated successfully"}"#.into(),
            "DELETE" => r#"{"deleted":true,"message":"Resource deleted successfully"}"#.into(),
            _ => r#"{"error":"Method not allowed"}"#.into(),
        };

        let status_code = match method {
            "POST" => 201,
            "DELETE" => 204,
            _ => 200,
        };

        return HttpResponse {
            status_code,
            status_text: status_text_for(status_code).into(),
            headers: response_headers,
            body,
            elapsed_ms: 45,
        };
    }

    if lower_url.contains("404") || lower_url.contains("notfound") {
        return HttpResponse {
            status_code: 404,
            status_text: "Not Found".into(),
            headers: response_headers,
            body: r#"{"error":"Not found"}"#.into(),
            elapsed_ms: 8,
        };
    }

    // Default: return a 200 with an HTML-like response
    response_headers.insert("content-type".into(), "text/html".into());
    HttpResponse {
        status_code: 200,
        status_text: "OK".into(),
        headers: response_headers,
        body: format!(
            "<html><body><h1>Mock Response</h1><p>Fetched {} {}</p></body></html>",
            method, url
        ),
        elapsed_ms: 30,
    }
}

fn status_text_for(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}

fn simple_hash(s: &str) -> u64 {
    s.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definition() {
        let tool = HttpRequestTool;
        assert_eq!(tool.name(), "http_request");
        let schema = tool.parameters_schema();
        assert_eq!(schema["required"], serde_json::json!(["url"]));
        assert!(schema["properties"]["method"].is_object());
        assert!(schema["properties"]["headers"].is_object());
        assert!(schema["properties"]["body"].is_object());
    }

    #[tokio::test]
    async fn get_request_returns_success() {
        let tool = HttpRequestTool;
        let result = tool
            .execute(serde_json::json!({
                "url": "https://example.com/page"
            }))
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.contains("Mock Response"));
    }

    #[tokio::test]
    async fn health_endpoint_returns_healthy() {
        let tool = HttpRequestTool;
        let result = tool
            .execute(serde_json::json!({
                "url": "https://myapp.com/health"
            }))
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.contains("healthy"));
    }

    #[tokio::test]
    async fn post_to_api_returns_created() {
        let tool = HttpRequestTool;
        let result = tool
            .execute(serde_json::json!({
                "url": "https://api.example.com/v1/items",
                "method": "POST",
                "body": "{\"name\": \"Test Item\"}"
            }))
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.contains("created"));
        let data = result.data.unwrap();
        assert_eq!(data["status_code"], 201);
    }

    #[tokio::test]
    async fn delete_api_resource() {
        let tool = HttpRequestTool;
        let result = tool
            .execute(serde_json::json!({
                "url": "https://api.example.com/v1/items/42",
                "method": "DELETE"
            }))
            .await
            .unwrap();

        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data["status_code"], 204);
    }

    #[tokio::test]
    async fn not_found_url_returns_404() {
        let tool = HttpRequestTool;
        let result = tool
            .execute(serde_json::json!({
                "url": "https://example.com/404"
            }))
            .await
            .unwrap();

        assert!(!result.success);
        let data = result.data.unwrap();
        assert_eq!(data["status_code"], 404);
    }

    #[tokio::test]
    async fn missing_url_returns_error() {
        let tool = HttpRequestTool;
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn invalid_url_scheme_returns_error() {
        let tool = HttpRequestTool;
        let result = tool
            .execute(serde_json::json!({ "url": "ftp://files.example.com" }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn invalid_method_returns_error() {
        let tool = HttpRequestTool;
        let result = tool
            .execute(serde_json::json!({
                "url": "https://example.com",
                "method": "TRACE"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn put_updates_resource() {
        let tool = HttpRequestTool;
        let result = tool
            .execute(serde_json::json!({
                "url": "https://api.example.com/v1/items/1",
                "method": "PUT",
                "body": "{\"name\": \"Updated\"}"
            }))
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.contains("updated"));
    }

    #[tokio::test]
    async fn custom_headers_accepted() {
        let tool = HttpRequestTool;
        let result = tool
            .execute(serde_json::json!({
                "url": "https://api.example.com/v1/data",
                "headers": {
                    "Authorization": "Bearer token123",
                    "Accept": "application/json"
                }
            }))
            .await
            .unwrap();

        assert!(result.success);
    }
}

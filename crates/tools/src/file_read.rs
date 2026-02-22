//! File read tool — read file contents with path validation.

use async_trait::async_trait;
use rustedclaw_core::error::ToolError;
use rustedclaw_core::tool::{Tool, ToolResult};

pub struct FileReadTool {
    /// Allowed root directories. Empty = allow all.
    pub allowed_roots: Vec<String>,
    /// Forbidden path prefixes.
    pub forbidden_paths: Vec<String>,
}

impl FileReadTool {
    /// Create a file read tool with no path restrictions.
    pub fn new() -> Self {
        Self {
            allowed_roots: Vec::new(),
            forbidden_paths: Vec::new(),
        }
    }

    /// Create a file read tool with path restrictions.
    pub fn with_restrictions(allowed_roots: Vec<String>, forbidden_paths: Vec<String>) -> Self {
        Self {
            allowed_roots,
            forbidden_paths,
        }
    }
}

impl Default for FileReadTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file at the given path."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The file path to read"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<ToolResult, ToolError> {
        let path = arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("Missing 'path' argument".into()))?;

        // Validate path against security policy
        if let Err(e) =
            rustedclaw_security::validate_path(path, &self.allowed_roots, &self.forbidden_paths)
        {
            return Err(ToolError::PermissionDenied {
                tool_name: "file_read".into(),
                reason: e.to_string(),
            });
        }

        match tokio::fs::read_to_string(path).await {
            Ok(content) => Ok(ToolResult {
                call_id: String::new(),
                success: true,
                output: content,
                data: None,
            }),
            Err(e) => Ok(ToolResult {
                call_id: String::new(),
                success: false,
                output: format!("Failed to read file: {e}"),
                data: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn tool_definition() {
        let tool = FileReadTool::new();
        assert_eq!(tool.name(), "file_read");
        let schema = tool.parameters_schema();
        assert_eq!(schema["required"], serde_json::json!(["path"]));
        assert!(schema["properties"]["path"].is_object());
    }

    #[tokio::test]
    async fn read_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(f, "Hello, world!").unwrap();

        let tool = FileReadTool::new();
        let result = tool
            .execute(serde_json::json!({
                "path": file_path.to_str().unwrap()
            }))
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.contains("Hello, world!"));
    }

    #[tokio::test]
    async fn read_nonexistent_file() {
        let tool = FileReadTool::new();
        let result = tool
            .execute(serde_json::json!({
                "path": "/tmp/rustedclaw_test_nonexistent_file_12345.txt"
            }))
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result.output.contains("Failed to read file"));
    }

    #[tokio::test]
    async fn missing_path_argument() {
        let tool = FileReadTool::new();
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn path_traversal_blocked() {
        let tool = FileReadTool::with_restrictions(vec!["/home/user/workspace".into()], vec![]);
        let result = tool
            .execute(serde_json::json!({
                "path": "../../../etc/passwd"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn forbidden_path_blocked() {
        let tool = FileReadTool::with_restrictions(vec![], vec!["/etc".into()]);
        let result = tool
            .execute(serde_json::json!({
                "path": "/etc/shadow"
            }))
            .await;
        assert!(result.is_err());
    }
}

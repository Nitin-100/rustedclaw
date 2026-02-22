//! File write tool — write or create files with path validation.

use async_trait::async_trait;
use rustedclaw_core::error::ToolError;
use rustedclaw_core::tool::{Tool, ToolResult};

pub struct FileWriteTool {
    /// Allowed root directories. Empty = allow all.
    pub allowed_roots: Vec<String>,
    /// Forbidden path prefixes.
    pub forbidden_paths: Vec<String>,
}

impl FileWriteTool {
    /// Create a file write tool with no path restrictions.
    pub fn new() -> Self {
        Self {
            allowed_roots: Vec::new(),
            forbidden_paths: Vec::new(),
        }
    }

    /// Create a file write tool with path restrictions.
    pub fn with_restrictions(allowed_roots: Vec<String>, forbidden_paths: Vec<String>) -> Self {
        Self {
            allowed_roots,
            forbidden_paths,
        }
    }
}

impl Default for FileWriteTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "file_write"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates the file if it doesn't exist, overwrites if it does."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The file path to write to"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<ToolResult, ToolError> {
        let path = arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("Missing 'path' argument".into()))?;

        let content = arguments["content"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("Missing 'content' argument".into()))?;

        // Validate path against security policy
        if let Err(e) =
            rustedclaw_security::validate_path(path, &self.allowed_roots, &self.forbidden_paths)
        {
            return Err(ToolError::PermissionDenied {
                tool_name: "file_write".into(),
                reason: e.to_string(),
            });
        }

        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(path).parent()
            && let Err(e) = tokio::fs::create_dir_all(parent).await
        {
            return Ok(ToolResult {
                call_id: String::new(),
                success: false,
                output: format!("Failed to create directory: {e}"),
                data: None,
            });
        }

        match tokio::fs::write(path, content).await {
            Ok(()) => Ok(ToolResult {
                call_id: String::new(),
                success: true,
                output: format!("Successfully wrote {} bytes to {path}", content.len()),
                data: None,
            }),
            Err(e) => Ok(ToolResult {
                call_id: String::new(),
                success: false,
                output: format!("Failed to write file: {e}"),
                data: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definition() {
        let tool = FileWriteTool::new();
        assert_eq!(tool.name(), "file_write");
        let schema = tool.parameters_schema();
        assert_eq!(schema["required"], serde_json::json!(["path", "content"]));
        assert!(schema["properties"]["path"].is_object());
        assert!(schema["properties"]["content"].is_object());
    }

    #[tokio::test]
    async fn write_and_verify() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("output.txt");

        let tool = FileWriteTool::new();
        let result = tool
            .execute(serde_json::json!({
                "path": file_path.to_str().unwrap(),
                "content": "Hello from test!"
            }))
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.contains("16 bytes"));

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "Hello from test!");
    }

    #[tokio::test]
    async fn write_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("nested").join("dir").join("file.txt");

        let tool = FileWriteTool::new();
        let result = tool
            .execute(serde_json::json!({
                "path": file_path.to_str().unwrap(),
                "content": "nested content"
            }))
            .await
            .unwrap();

        assert!(result.success);
        assert!(file_path.exists());
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "nested content");
    }

    #[tokio::test]
    async fn overwrite_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("overwrite.txt");
        std::fs::write(&file_path, "old content").unwrap();

        let tool = FileWriteTool::new();
        let result = tool
            .execute(serde_json::json!({
                "path": file_path.to_str().unwrap(),
                "content": "new content"
            }))
            .await
            .unwrap();

        assert!(result.success);
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "new content");
    }

    #[tokio::test]
    async fn missing_path_argument() {
        let tool = FileWriteTool::new();
        let result = tool
            .execute(serde_json::json!({ "content": "hello" }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn missing_content_argument() {
        let tool = FileWriteTool::new();
        let result = tool
            .execute(serde_json::json!({ "path": "/tmp/test.txt" }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn path_traversal_blocked() {
        let tool = FileWriteTool::with_restrictions(vec!["/home/user/workspace".into()], vec![]);
        let result = tool
            .execute(serde_json::json!({
                "path": "../../../etc/crontab",
                "content": "malicious"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn forbidden_path_blocked() {
        let tool = FileWriteTool::with_restrictions(vec![], vec!["/etc".into()]);
        let result = tool
            .execute(serde_json::json!({
                "path": "/etc/shadow",
                "content": "malicious"
            }))
            .await;
        assert!(result.is_err());
    }
}

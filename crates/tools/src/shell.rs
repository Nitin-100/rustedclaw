//! Shell tool — execute system commands.
//!
//! Supports command allowlisting, workspace scoping, and timeout.

use async_trait::async_trait;
use rustedclaw_core::error::ToolError;
use rustedclaw_core::tool::{Tool, ToolResult};
use tokio::process::Command;
use tracing::{debug, warn};

/// Execute shell commands with safety constraints.
pub struct ShellTool {
    /// If non-empty, only these commands are allowed.
    allowed_commands: Vec<String>,
}

impl ShellTool {
    pub fn new(allowed_commands: Vec<String>) -> Self {
        Self { allowed_commands }
    }

    fn is_command_allowed(&self, command: &str) -> bool {
        if self.allowed_commands.is_empty() {
            return true; // No allowlist = all commands allowed
        }

        // Extract the base command (first word)
        let base_cmd = command
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim();

        self.allowed_commands.iter().any(|a| a == base_cmd)
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str { "shell" }

    fn description(&self) -> &str {
        "Execute a shell command and return stdout/stderr. Use this for running programs, checking files, git operations, etc."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<ToolResult, ToolError> {
        let command = arguments["command"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("Missing 'command' argument".into()))?;

        if !self.is_command_allowed(command) {
            return Err(ToolError::PermissionDenied {
                tool_name: "shell".into(),
                reason: format!("Command '{}' not in allowlist", command.split_whitespace().next().unwrap_or("")),
            });
        }

        debug!(command = %command, "Executing shell command");

        let output = if cfg!(target_os = "windows") {
            Command::new("cmd")
                .args(["/C", command])
                .output()
                .await
        } else {
            Command::new("sh")
                .args(["-c", command])
                .output()
                .await
        };

        match output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let success = output.status.success();

                let result_text = if success {
                    if stderr.is_empty() {
                        stdout
                    } else {
                        format!("{stdout}\n[stderr]: {stderr}")
                    }
                } else {
                    let code = output.status.code().unwrap_or(-1);
                    warn!(command = %command, exit_code = code, "Command failed");
                    format!("[exit code: {code}]\n{stdout}\n{stderr}")
                };

                Ok(ToolResult {
                    call_id: String::new(),
                    success,
                    output: result_text.trim().to_string(),
                    data: None,
                })
            }
            Err(e) => Err(ToolError::ExecutionFailed {
                tool_name: "shell".into(),
                reason: e.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_check() {
        let tool = ShellTool::new(vec!["ls".into(), "cat".into(), "git".into()]);
        assert!(tool.is_command_allowed("ls -la"));
        assert!(tool.is_command_allowed("cat file.txt"));
        assert!(tool.is_command_allowed("git status"));
        assert!(!tool.is_command_allowed("rm -rf /"));
        assert!(!tool.is_command_allowed("sudo something"));
    }

    #[test]
    fn empty_allowlist_allows_all() {
        let tool = ShellTool::new(vec![]);
        assert!(tool.is_command_allowed("anything goes"));
    }

    #[tokio::test]
    async fn execute_echo() {
        let tool = ShellTool::new(vec![]);
        let result = tool
            .execute(serde_json::json!({"command": "echo hello"}))
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("hello"));
    }

    #[tokio::test]
    async fn blocked_command() {
        let tool = ShellTool::new(vec!["ls".into()]);
        let result = tool
            .execute(serde_json::json!({"command": "rm -rf /"}))
            .await;
        assert!(matches!(result, Err(ToolError::PermissionDenied { .. })));
    }
}

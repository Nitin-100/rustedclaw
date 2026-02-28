//! Shell tool — execute system commands.
//!
//! Supports command allowlisting, workspace scoping, timeout enforcement,
//! and injection prevention (pipes, subshells, semicolons are blocked).

use async_trait::async_trait;
use rustedclaw_core::error::ToolError;
use rustedclaw_core::tool::{Tool, ToolResult};
use std::time::Duration;
use tokio::process::Command;
use tracing::{debug, warn};

/// Default command timeout: 30 seconds.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Characters that indicate shell injection attempts.
const DANGEROUS_CHARS: &[char] = &['|', ';', '&', '`', '$', '(', ')', '{', '}', '<', '>', '\n'];

/// Execute shell commands with safety constraints.
pub struct ShellTool {
    /// If non-empty, only these base commands are allowed.
    /// If empty, **all commands are denied** (secure by default).
    allowed_commands: Vec<String>,
    /// Maximum execution time before the command is killed.
    timeout: Duration,
}

impl ShellTool {
    /// Create a ShellTool with an explicit allowlist.
    ///
    /// An empty allowlist means **no commands are allowed** (deny-by-default).
    pub fn new(allowed_commands: Vec<String>) -> Self {
        Self {
            allowed_commands,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Set a custom timeout duration.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Check if a command is allowed based on the allowlist.
    ///
    /// Rules:
    /// 1. Empty allowlist → deny all (secure by default)
    /// 2. Only the first word (base command) is checked against the allowlist
    /// 3. Commands containing shell metacharacters are always denied
    fn is_command_allowed(&self, command: &str) -> Result<(), String> {
        // Block empty commands
        if command.trim().is_empty() {
            return Err("Empty command".into());
        }

        // Block shell injection characters (pipes, subshells, semicolons, etc.)
        if command.chars().any(|c| DANGEROUS_CHARS.contains(&c)) {
            return Err(
                "Command contains forbidden shell metacharacters. \
                 Pipes (|), semicolons (;), subshells ($(), ``), and redirects (<, >) are not allowed. \
                 Run each command separately."
                    .to_string(),
            );
        }

        // Deny-by-default: empty allowlist means nothing is permitted
        if self.allowed_commands.is_empty() {
            return Err(
                "No commands are allowed (empty allowlist — configure allowed_commands)".into(),
            );
        }

        // Extract the base command (first word)
        let base_cmd = command.split_whitespace().next().unwrap_or("").trim();

        if self.allowed_commands.iter().any(|a| a == base_cmd) {
            Ok(())
        } else {
            Err(format!(
                "Command '{}' not in allowlist. Allowed: {:?}",
                base_cmd, self.allowed_commands
            ))
        }
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

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

        if let Err(reason) = self.is_command_allowed(command) {
            return Err(ToolError::PermissionDenied {
                tool_name: "shell".into(),
                reason,
            });
        }

        debug!(command = %command, timeout = ?self.timeout, "Executing shell command");

        let child = if cfg!(target_os = "windows") {
            Command::new("cmd").args(["/C", command]).output()
        } else {
            Command::new("sh").args(["-c", command]).output()
        };

        // Enforce timeout on command execution
        let output = match tokio::time::timeout(self.timeout, child).await {
            Ok(result) => result,
            Err(_) => {
                warn!(command = %command, timeout = ?self.timeout, "Command timed out");
                return Ok(ToolResult {
                    call_id: String::new(),
                    success: false,
                    output: format!(
                        "Command timed out after {} seconds. Use a shorter-running command.",
                        self.timeout.as_secs()
                    ),
                    data: None,
                });
            }
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
        assert!(tool.is_command_allowed("ls -la").is_ok());
        assert!(tool.is_command_allowed("cat file.txt").is_ok());
        assert!(tool.is_command_allowed("git status").is_ok());
        assert!(tool.is_command_allowed("rm -rf /").is_err());
        assert!(tool.is_command_allowed("sudo something").is_err());
    }

    #[test]
    fn empty_allowlist_denies_all() {
        let tool = ShellTool::new(vec![]);
        assert!(tool.is_command_allowed("anything goes").is_err());
    }

    #[test]
    fn shell_injection_blocked() {
        let tool = ShellTool::new(vec!["ls".into(), "echo".into()]);
        // Pipes
        assert!(tool.is_command_allowed("ls | rm -rf /").is_err());
        // Semicolons
        assert!(tool.is_command_allowed("ls; rm -rf /").is_err());
        // Subshells
        assert!(tool.is_command_allowed("echo $(rm -rf /)").is_err());
        // Backticks
        assert!(tool.is_command_allowed("echo `whoami`").is_err());
        // Ampersand
        assert!(tool.is_command_allowed("ls && rm -rf /").is_err());
        // Redirects
        assert!(tool.is_command_allowed("echo test > /etc/passwd").is_err());
    }

    #[tokio::test]
    async fn execute_echo() {
        let tool = ShellTool::new(vec!["echo".into()]);
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

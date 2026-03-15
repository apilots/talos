use async_trait::async_trait;
use serde_json::Value;
use std::time::{Duration, Instant};
use talos_error::TalosResult;
use tokio::process::Command;
use tokio::time;

use crate::{Tool, ToolContext, ToolOutput};

/// Tool for executing shell commands
pub struct ExecTool;

#[async_trait]
impl Tool for ExecTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Executes a shell command. Input should contain 'command' field."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["command"],
            "properties": {
                "command": {"type": "string"},
                "cwd": {"type": "string"},
                "timeout_secs": {"type": "integer", "minimum": 1}
            }
        }))
    }

    async fn execute(&self, input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
        let cmd = if let Some(cmd_val) = input.get("command") {
            cmd_val.as_str().unwrap_or("").to_string()
        } else if let Some(cmd_val) = input.get("cmd") {
            cmd_val.as_str().unwrap_or("").to_string()
        } else if input.is_string() {
            input.as_str().unwrap_or("").to_string()
        } else {
            return Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some("Input must contain 'command' field or be a string".to_string()),
            });
        };

        if cmd.trim().is_empty() {
            return Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some("Command cannot be empty".to_string()),
            });
        }

        let cwd = input
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string());
        let timeout_secs = input
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(30);

        let mut command = Command::new("sh");
        command.arg("-c").arg(&cmd);
        if let Some(path) = cwd {
            command.current_dir(path);
        }

        let started_at = Instant::now();
        let result = time::timeout(Duration::from_secs(timeout_secs), command.output()).await;

        match result {
            Ok(Ok(output)) => {
                let elapsed_ms = started_at.elapsed().as_millis();
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(-1);

                if output.status.success() {
                    Ok(ToolOutput {
                        success: true,
                        output: stdout,
                        error: None,
                    })
                } else {
                    Ok(ToolOutput {
                        success: false,
                        output: stdout,
                        error: Some(format!(
                            "Command failed (exit_code={}, duration_ms={}): {}",
                            exit_code, elapsed_ms, stderr
                        )),
                    })
                }
            }
            Ok(Err(err)) => Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some(format!("Failed to execute command '{}': {}", cmd, err)),
            }),
            Err(_) => Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some(format!("Command timed out after {} seconds", timeout_secs)),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_exec_tool_success() {
        let tool = ExecTool;
        let input = serde_json::json!({
            "command": "echo 'Hello, World!'"
        });
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(result.success);
        assert!(result.output.trim() == "Hello, World!");
    }

    #[tokio::test]
    async fn test_exec_tool_failure() {
        let tool = ExecTool;
        let input = serde_json::json!({
            "command": "nonexistent_command_12345"
        });
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(!result.success);
        // Either output or error should contain information about the failure
        assert!(
            result.output.contains("nonexistent_command_12345")
                || result
                    .error
                    .as_ref()
                    .unwrap()
                    .contains("nonexistent_command_12345")
        );
    }

    #[tokio::test]
    async fn test_exec_tool_with_error_output() {
        let tool = ExecTool;
        let input = serde_json::json!({
            "command": "ls /nonexistent_directory_12345"
        });
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        // This command should fail, so success is false but we might have output in either field
        // depending on how the system handles the error
        assert!(!result.success || result.output.is_empty());
    }

    #[tokio::test]
    async fn test_exec_tool_timeout() {
        let tool = ExecTool;
        let input = serde_json::json!({
            "command": "sleep 2",
            "timeout_secs": 1
        });
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(!result.success);
        assert!(result.error.unwrap_or_default().contains("timed out"));
    }
}

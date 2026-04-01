use async_trait::async_trait;
use serde_json::Value;
use talos_error::TalosResult;

use crate::runtimes::exec::{exec_command, write_stdin, ExecCommandRequest, WriteStdinRequest};
use crate::{Tool, ToolContext, ToolOutput};

pub struct ExecCommandTool;

pub struct WriteStdinTool;

#[async_trait]
impl Tool for ExecCommandTool {
    fn name(&self) -> &str {
        "exec_command"
    }

    fn description(&self) -> &str {
        "Run a shell command and optionally keep the session open for later stdin writes."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["cmd"],
            "properties": {
                "cmd": { "type": "string" },
                "workdir": { "type": "string" },
                "shell": { "type": "string" },
                "login": { "type": "boolean" },
                "tty": { "type": "boolean" },
                "yield_time_ms": { "type": "integer", "minimum": 0 },
                "max_output_tokens": { "type": "integer", "minimum": 1 },
                "sandbox_permissions": { "type": "string", "enum": ["use_default", "require_escalated"] },
                "justification": { "type": "string" },
                "prefix_rule": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            }
        }))
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let request: ExecCommandRequest = serde_json::from_value(input).map_err(|err| {
            talos_error::TalosError::ToolError(format!("invalid exec_command arguments: {}", err))
        })?;
        let response = exec_command(&request, &context).await?;
        let output = serde_json::to_string(&response).map_err(|err| {
            talos_error::TalosError::ToolError(format!("serialize exec_command response: {}", err))
        })?;
        Ok(ToolOutput {
            success: true,
            output,
            error: None,
        })
    }
}

#[async_trait]
impl Tool for WriteStdinTool {
    fn name(&self) -> &str {
        "write_stdin"
    }

    fn description(&self) -> &str {
        "Write stdin to a prior exec_command session and return recent output."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["session_id"],
            "properties": {
                "session_id": { "type": "integer" },
                "chars": { "type": "string" },
                "yield_time_ms": { "type": "integer", "minimum": 0 },
                "max_output_tokens": { "type": "integer", "minimum": 1 }
            }
        }))
    }

    async fn execute(&self, input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
        let request: WriteStdinRequest = serde_json::from_value(input).map_err(|err| {
            talos_error::TalosError::ToolError(format!("invalid write_stdin arguments: {}", err))
        })?;
        let response = write_stdin(&request).await?;
        let output = serde_json::to_string(&response).map_err(|err| {
            talos_error::TalosError::ToolError(format!("serialize write_stdin response: {}", err))
        })?;
        Ok(ToolOutput {
            success: true,
            output,
            error: None,
        })
    }
}

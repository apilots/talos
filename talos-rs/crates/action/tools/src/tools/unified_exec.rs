use async_trait::async_trait;
use serde_json::Value;
use talos_error::TalosResult;

use crate::runtimes::exec::{
    exec_command, write_stdin_with_context, ExecCommandRequest, WriteStdinRequest,
};
use crate::{Tool, ToolContext, ToolOutput};

pub struct ExecCommandTool;

pub struct WriteStdinTool;

#[async_trait]
impl Tool for ExecCommandTool {
    fn name(&self) -> &str {
        "exec_command"
    }

    fn description(&self) -> &str {
        "Run a shell command when shell execution itself is the task, such as running tests, build tools, git commands, scripts, REPLs, or interactive programs. Do not use this as a substitute for repository search or file inspection.\n\nUsage:\n- Prefer read_file over exec_command with cat, head, or tail\n- Prefer list_dir over exec_command with ls or find\n- Prefer grep_files over exec_command with grep or rg\n- Prefer edit or write over exec_command with sed, awk, or heredoc writes\n- workdir defaults to the session project root when omitted\n- For commands that require elevated permissions, set sandbox_permissions to require_escalated and provide a justification\n\nIMPORTANT: Reserve exec_command for cases where running a command is genuinely necessary."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["cmd"],
            "properties": {
                "cmd": {
                    "type": "string",
                    "description": "Shell command to execute."
                },
                "workdir": {
                    "type": "string",
                    "description": "Optional working directory for the command. Defaults to the session workspace when omitted."
                },
                "shell": {
                    "type": "string",
                    "description": "Optional shell binary to launch instead of the default shell."
                },
                "login": {
                    "type": "boolean",
                    "description": "Whether to start the shell with login semantics."
                },
                "tty": {
                    "type": "boolean",
                    "description": "Whether to allocate a TTY and keep the session interactive."
                },
                "yield_time_ms": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "How long to wait for output before returning control, in milliseconds."
                },
                "max_output_tokens": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum number of output tokens to return in the response."
                },
                "sandbox_permissions": {
                    "type": "string",
                    "enum": ["use_default", "require_escalated"],
                    "description": "Sandbox mode for the command: use_default runs with normal restrictions, require_escalated requests elevated permissions."
                },
                "justification": {
                    "type": "string",
                    "description": "Short explanation of why elevated permissions are needed when sandbox_permissions is require_escalated."
                },
                "prefix_rule": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional command prefix pattern that can authorize similar elevated commands in future runs."
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

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let request: WriteStdinRequest = serde_json::from_value(input).map_err(|err| {
            talos_error::TalosError::ToolError(format!("invalid write_stdin arguments: {}", err))
        })?;
        let response = write_stdin_with_context(&request, &context).await?;
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

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::runtimes::bash::{run_bash_command, BashCommandRequest, SandboxPermissions};
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy, ToolVisibility};
use crate::{Tool, ToolContext, ToolOutput};

pub struct BashTool;

#[derive(Debug, Deserialize)]
struct BashInput {
    command: String,
    #[serde(default)]
    timeout: Option<u64>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    run_in_background: bool,
    #[serde(default)]
    dangerously_disable_sandbox: bool,
    #[serde(default)]
    workdir: Option<String>,
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "Bash"
    }

    fn internal_name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Run a shell command.\n\nUsage:\n- Use Bash for running tests, builds, git commands, scripts, and other shell-only tasks.\n- Use Read, Grep, Edit, Write, and Glob instead of cat/head/tail, grep/rg, sed/awk, shell heredoc writes, or find when dedicated tools can answer directly.\n- Use run_in_background when you do not need the result immediately and are okay checking it later with TaskOutput or stopping it with TaskStop.\n- If commands are independent and can run in parallel, issue multiple Bash tool calls in a single message."
    }

    fn visibility(&self) -> ToolVisibility {
        ToolVisibility::DefaultVisible
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["command"],
            "properties": {
                "command": { "type": "string" },
                "timeout": { "type": "integer", "minimum": 0 },
                "description": { "type": "string" },
                "run_in_background": { "type": "boolean" },
                "dangerously_disable_sandbox": { "type": "boolean" },
                "workdir": { "type": "string" }
            }
        }))
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        ToolBehaviorFlags {
            read_only: false,
            destructive: true,
            concurrency_safe: false,
            requires_user_interaction: false,
        }
    }

    fn result_policy(&self) -> ToolResultPolicy {
        ToolResultPolicy {
            max_inline_bytes: 32 * 1024,
            persist_large_output: true,
            preview_lines: 60,
        }
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let input: BashInput = serde_json::from_value(input)
            .map_err(|err| TalosError::ToolError(format!("invalid Bash arguments: {}", err)))?;
        let request = BashCommandRequest {
            cmd: input.command,
            workdir: input.workdir,
            shell: None,
            login: true,
            tty: input.run_in_background,
            yield_time_ms: input.timeout.unwrap_or(10_000),
            max_output_tokens: Some(8_000),
            sandbox_permissions: if input.dangerously_disable_sandbox {
                SandboxPermissions::RequireEscalated
            } else {
                SandboxPermissions::UseDefault
            },
            justification: input.description,
            prefix_rule: None,
        };
        let response = run_bash_command(&request, &context).await?;
        Ok(ToolOutput {
            success: true,
            output: serde_json::to_string(&response).map_err(|err| {
                TalosError::ToolError(format!("serialize Bash response: {}", err))
            })?,
            error: None,
        })
    }
}

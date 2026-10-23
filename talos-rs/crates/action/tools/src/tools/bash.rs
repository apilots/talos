use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_tools_sandbox::{SandboxCapability, ToolSandboxOperationKind};
use talos_types::talos::{TalosContentItem, TalosToolTaskStatus};

use crate::runtimes::bash::{
    run_bash_command_with_policy, BashCommandRequest, BashRunStatus, SandboxPermissions,
};
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy, ToolVisibility};
use crate::{Tool, ToolContext, ToolOutputPayload};

pub struct BashTool;

const DEFAULT_BASH_TIMEOUT_MS: u64 = 120_000;
const BASH_TIMEOUT_RESULT_GRACE_MS: u64 = 250;

#[derive(Debug, Deserialize)]
struct BashInput {
    command: String,
    #[serde(default)]
    timeout: Option<u64>,
    #[serde(default)]
    description: Option<String>,
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
        "Run a shell command.\n\nUsage:\n- Use Bash for running tests, builds, git commands, scripts, and other shell-only tasks.\n- Use Read, Grep, Edit, Write, and Glob instead of cat/head/tail, grep/rg, sed/awk, shell heredoc writes, or find when dedicated tools can answer directly.\n- Output previews are bounded by configured bytes and lines. Large output is saved on this Computer; use Grep or Read with offset/limit or byte_offset/max_bytes to inspect the returned path. Do not rerun a completed command just to recover its output.\n- Set timeout to the maximum command runtime in milliseconds.\n- If commands are independent and can run in parallel, issue multiple Bash tool calls in a single message."
    }

    fn visibility(&self) -> ToolVisibility {
        ToolVisibility::DefaultVisible
    }

    fn sandbox_operation(&self) -> Option<ToolSandboxOperationKind> {
        Some(ToolSandboxOperationKind::SpawnProcess)
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["command"],
            "properties": {
                "command": { "type": "string" },
                "timeout": { "type": "integer", "minimum": 1 },
                "description": { "type": "string" },
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

    fn validate_input(&self, input: &Value) -> TalosResult<()> {
        parse_bash_input(input)?;
        Ok(())
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let input = parse_bash_input(&input)?;
        let timeout_ms = input.timeout.unwrap_or(DEFAULT_BASH_TIMEOUT_MS);
        let sandbox_enabled =
            context.sandbox_permit()?.effective_policy().sandbox == SandboxCapability::Enabled;
        let request = BashCommandRequest {
            cmd: input.command,
            workdir: input.workdir,
            shell: sandbox_enabled.then(|| "/bin/sh".to_string()),
            login: !sandbox_enabled,
            tty: false,
            yield_time_ms: timeout_ms.saturating_add(BASH_TIMEOUT_RESULT_GRACE_MS),
            timeout_ms: Some(timeout_ms),
            max_output_bytes: None,
            sandbox_permissions: if input.dangerously_disable_sandbox {
                SandboxPermissions::RequireEscalated
            } else {
                SandboxPermissions::UseDefault
            },
            justification: input.description,
            prefix_rule: None,
        };
        let mut policy = self.result_policy();
        let config = context
            .kernel_config()
            .ok_or_else(|| TalosError::internal("Bash requires effective Kernel configuration."))?;
        policy.max_inline_bytes = policy
            .max_inline_bytes
            .min(config.tools.bash.max_output_bytes)
            .min(config.tool_result_max_bytes)
            .min(crate::output_budget::MAX_MODEL_TOOL_BYTES);
        if policy.max_inline_bytes < 4096 {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Bash output budget must be at least 4096 bytes to preserve status and log references.",
            ));
        }
        policy.max_inline_bytes -= 2048;
        let response = run_bash_command_with_policy(&request, &context, policy).await?;
        let mut structured = serde_json::to_value(&response).map_err(|err| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("serialize Bash response: {}", err),
            )
        })?;
        let status = match response.status {
            BashRunStatus::Running => TalosToolTaskStatus::Running,
            BashRunStatus::TimedOut => TalosToolTaskStatus::TimedOut,
            BashRunStatus::Cancelled => TalosToolTaskStatus::Cancelled,
            BashRunStatus::Exited => {
                if response.exit_code == Some(0) {
                    TalosToolTaskStatus::Completed
                } else {
                    TalosToolTaskStatus::Failed
                }
            }
        };
        if let Some(error) = &response.capture_error {
            structured["error"] = Value::String(error.message.clone());
            structured["retryable"] = Value::Bool(false);
            structured["suggested_action"] = Value::String(
                "Inspect the command status and captured output. Do not rerun the command automatically.".into(),
            );
        } else {
            // Error prose must not fall back to the whole serialized response,
            // duplicating the preview and log reference in the model message.
            let failure = match status {
                TalosToolTaskStatus::Failed => Some(response.exit_code.map_or_else(
                    || "Bash command ended without a successful exit code.".into(),
                    |code| format!("Bash command failed with exit code {code}."),
                )),
                TalosToolTaskStatus::TimedOut => Some("Bash command exceeded its timeout.".into()),
                TalosToolTaskStatus::Cancelled => Some("Bash command was cancelled.".into()),
                TalosToolTaskStatus::Running | TalosToolTaskStatus::Queued => {
                    Some("Bash command is still running.".into())
                }
                TalosToolTaskStatus::Completed => None,
            };
            if let Some(message) = failure {
                structured["error"] = Value::String(message);
            }
        }
        let (stdout, stderr) = bash_output_chunk(&response);
        let content_items = vec![TalosContentItem::BashOutput {
            session_id: response.session_id,
            status,
            exit_code: response.exit_code,
            stdout,
            stderr,
            truncated: response.truncated,
            elapsed_ms: Some(response.elapsed_ms),
            output_log: response.output_log.clone(),
        }];
        if status == TalosToolTaskStatus::Completed && response.capture_error.is_none() {
            Ok(ToolOutputPayload::success_with_content_items(
                structured,
                content_items,
            ))
        } else {
            Ok(ToolOutputPayload::failure_with_content_items(
                structured,
                content_items,
            ))
        }
    }
}

fn bash_output_chunk(response: &crate::runtimes::bash::BashSessionResponse) -> (String, String) {
    (response.stdout.clone(), response.stderr.clone())
}

fn parse_bash_input(input: &Value) -> TalosResult<BashInput> {
    let parsed: BashInput = serde_json::from_value(input.clone()).map_err(|err| {
        TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!("invalid Bash arguments: {err}"),
        )
    })?;
    if parsed.command.trim().is_empty() {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "command must not be empty".to_string(),
        ));
    }
    Ok(parsed)
}

#[cfg(test)]
#[path = "bash_tests.rs"]
mod tests;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::runtimes::bash::{run_bash_command, BashCommandRequest, SandboxPermissions};
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy, ToolVisibility};
use crate::{Tool, ToolContext, ToolOutputPayload};

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

    fn validate_input(&self, input: &Value) -> TalosResult<()> {
        parse_bash_input(input)?;
        Ok(())
    }

    fn check_permissions(&self, input: &Value, context: &ToolContext) -> TalosResult<()> {
        let input = parse_bash_input(input)?;
        if input.dangerously_disable_sandbox && !context.pre_approved() {
            return Err(TalosError::AuthorizationDenied);
        }
        Ok(())
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let input = parse_bash_input(&input)?;
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
        let structured = serde_json::to_value(&response)
            .map_err(|err| TalosError::ToolError(format!("serialize Bash response: {}", err)))?;
        let content = serde_json::to_string(&response)
            .map_err(|err| TalosError::ToolError(format!("serialize Bash response: {}", err)))?;
        Ok(ToolOutputPayload::Function {
            content,
            success: true,
            structured_content: Some(structured),
        })
    }
}

fn parse_bash_input(input: &Value) -> TalosResult<BashInput> {
    let parsed: BashInput = serde_json::from_value(input.clone())
        .map_err(|err| TalosError::ValidationError(format!("invalid Bash arguments: {err}")))?;
    if parsed.command.trim().is_empty() {
        return Err(TalosError::ValidationError(
            "command must not be empty".to_string(),
        ));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_input_rejects_missing_command() {
        let tool = BashTool;
        let error = tool
            .validate_input(&serde_json::json!({"timeout": 10}))
            .unwrap_err();

        assert!(matches!(error, TalosError::ValidationError(_)));
    }

    #[test]
    fn check_permissions_rejects_escalation_without_preapproval() {
        let tool = BashTool;
        let context = ToolContext::default();
        let input = serde_json::json!({
            "command": "echo hi",
            "dangerously_disable_sandbox": true
        });

        let error = tool.check_permissions(&input, &context).unwrap_err();
        assert!(matches!(error, TalosError::AuthorizationDenied));
    }

    #[test]
    fn check_permissions_allows_escalation_with_preapproval() {
        let tool = BashTool;
        let context = ToolContext::default().with_pre_approved();
        let input = serde_json::json!({
            "command": "echo hi",
            "dangerously_disable_sandbox": true
        });

        assert!(tool.check_permissions(&input, &context).is_ok());
    }
}

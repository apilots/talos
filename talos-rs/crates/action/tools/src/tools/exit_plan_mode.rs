use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::runtimes::task_state_runtime::TaskStateRuntime;
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy, ToolVisibility};
use crate::state::PermissionMode;
use crate::{decode_tool_input, Tool, ToolContext, ToolOutput};

pub struct ExitPlanModeTool;

#[derive(Debug, Deserialize)]
struct ExitPlanModeRequest {
    #[serde(default)]
    summary: Option<String>,
}

#[derive(Debug, Serialize)]
struct PlanModeResponse {
    mode: PermissionMode,
    summary: Option<String>,
}

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str {
        "ExitPlanMode"
    }

    fn internal_name(&self) -> &str {
        "exit_plan_mode"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["exit_plan_mode"]
    }

    fn description(&self) -> &str {
        "Present the plan for approval and exit plan mode so implementation can begin."
    }

    fn visibility(&self) -> ToolVisibility {
        ToolVisibility::DefaultVisible
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        ToolBehaviorFlags {
            read_only: false,
            destructive: false,
            concurrency_safe: false,
            requires_user_interaction: false,
        }
    }

    fn result_policy(&self) -> ToolResultPolicy {
        ToolResultPolicy {
            max_inline_bytes: 4 * 1024,
            persist_large_output: false,
            preview_lines: 10,
        }
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string" }
            }
        }))
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let request: ExitPlanModeRequest = decode_tool_input(input, self.name())?;
        let mode = TaskStateRuntime::global().exit_plan_mode(context.session_id());
        let output = serde_json::to_string(&PlanModeResponse {
            mode,
            summary: request.summary,
        })
        .map_err(|err| TalosError::ToolError(format!("serialize exit_plan_mode: {err}")))?;
        Ok(ToolOutput {
            success: true,
            output,
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtimes::task_state_runtime::TaskStateRuntime;
    use crate::state::PermissionMode;
    use crate::EnterPlanModeTool;
    use uuid::Uuid;

    #[tokio::test]
    async fn exit_plan_mode_restores_default_permission_mode() {
        let enter_tool = EnterPlanModeTool;
        let exit_tool = ExitPlanModeTool;
        let session_id = format!("exit-plan-{}", Uuid::new_v4());

        enter_tool
            .execute(
                serde_json::json!({}),
                ToolContext::new(&session_id, "user-1"),
            )
            .await
            .expect("enter_plan_mode should succeed");

        let output = exit_tool
            .execute(
                serde_json::json!({ "summary": "Planning finished" }),
                ToolContext::new(&session_id, "user-1"),
            )
            .await
            .expect("exit_plan_mode should succeed");

        assert!(output.success);
        assert_eq!(
            TaskStateRuntime::global().current_permission_mode(&session_id),
            PermissionMode::Default
        );
        assert!(output.output.contains("\"mode\":\"default\""));
    }
}

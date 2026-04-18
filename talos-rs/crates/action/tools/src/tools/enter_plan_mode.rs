use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::runtimes::task_state_runtime::TaskStateRuntime;
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy, ToolVisibility};
use crate::state::PermissionMode;
use crate::{decode_tool_input, Tool, ToolContext, ToolOutput};

pub struct EnterPlanModeTool;

#[derive(Debug, Deserialize)]
struct EnterPlanModeRequest {
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct PlanModeResponse {
    mode: PermissionMode,
    reason: Option<String>,
}

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str {
        "EnterPlanMode"
    }

    fn internal_name(&self) -> &str {
        "enter_plan_mode"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["enter_plan_mode"]
    }

    fn description(&self) -> &str {
        "Request to enter plan mode for complex tasks that require exploration and design before coding."
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
                "reason": { "type": "string" }
            }
        }))
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let request: EnterPlanModeRequest = decode_tool_input(input, self.name())?;
        let mode = TaskStateRuntime::global().enter_plan_mode(context.session_id());

        // TODO(main-thread): feed PermissionMode::Plan into registry/orchestrator
        // governance so plan mode affects approval policy rather than being state-only.

        let output = serde_json::to_string(&PlanModeResponse {
            mode,
            reason: request.reason,
        })
        .map_err(|err| TalosError::ToolError(format!("serialize enter_plan_mode: {err}")))?;
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
    use uuid::Uuid;

    #[tokio::test]
    async fn enter_plan_mode_sets_permission_mode() {
        let tool = EnterPlanModeTool;
        let session_id = format!("enter-plan-{}", Uuid::new_v4());

        let output = tool
            .execute(
                serde_json::json!({ "reason": "Need explicit planning" }),
                ToolContext::new(&session_id, "user-1"),
            )
            .await
            .expect("enter_plan_mode should succeed");

        assert!(output.success);
        assert_eq!(
            TaskStateRuntime::global().current_permission_mode(&session_id),
            PermissionMode::Plan
        );
        assert!(output.output.contains("\"mode\":\"plan\""));
    }
}
